//! Shared axum state.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use plc_auth::{AuditEvent, AuditSink, AuthService, Clock, SystemClock};
use plc_config::DeviceConfig;
use plc_runtime::Runtime;
use plc_scan::{EpochHooks, ScanHandle, ScanStatusSnapshot};
use plc_types::OperatingMode;
use tokio::sync::Semaphore;

use crate::audit_log::RotatingAudit;
use crate::events::EventRing;
use crate::force_limit::ForceLimiter;
use crate::program_store::ProgramStore;

/// In-flight activate correlator.
#[derive(Debug, Clone)]
pub struct ActivateJob {
    /// UUID.
    pub job_id: String,
    /// Target program id.
    pub program_id: String,
}

/// Process-wide API state (Clone for axum).
#[derive(Clone)]
pub struct AppState {
    /// Dual-buffer runtime (brief std mutex; never hold across `.await`).
    pub runtime: Arc<Mutex<Runtime>>,
    /// Lock-free mode requests.
    pub scan_handle: ScanHandle,
    /// Atomic program phase.
    pub hooks: EpochHooks,
    /// Authn/authz (replaced on config write).
    pub auth: Arc<RwLock<AuthService>>,
    /// Live device config.
    pub config: Arc<RwLock<DeviceConfig>>,
    /// Path used by PUT/PATCH persist (`None` = memory only).
    pub config_path: Arc<Option<PathBuf>>,
    /// `.spkg` store.
    pub store: Arc<ProgramStore>,
    /// Rotating audit file.
    pub audit: Arc<RotatingAudit>,
    /// Diagnostics ring.
    pub events: Arc<EventRing>,
    /// Last scan snapshot copied into the diagnostics ring.
    diag_cursor: Arc<Mutex<ScanDiagCursor>>,
    /// Concurrent upload permit (1).
    pub upload_sem: Arc<Semaphore>,
    /// Serialize arm prepare/commit.
    pub arm_lock: Arc<tokio::sync::Mutex<()>>,
    /// Tag-force window.
    pub force_limit: Arc<Mutex<ForceLimiter>>,
    /// Process start.
    pub started: Instant,
    /// HTTP 2xx count.
    pub http_ok: Arc<AtomicU64>,
    /// HTTP non-2xx count.
    pub http_err: Arc<AtomicU64>,
    /// Last activate job.
    pub activate_job: Arc<Mutex<Option<ActivateJob>>>,
}

impl AppState {
    /// Assemble state around an existing [`Runtime`].
    pub fn new(
        cfg: DeviceConfig,
        runtime: Runtime,
        config_path: Option<PathBuf>,
    ) -> Result<Self, crate::error::ApiError> {
        let auth = AuthService::from_config(&cfg.auth, &cfg.limits)
            .map_err(|e| crate::error::ApiError::internal(e.to_string()))?;
        let scan_handle = runtime.engine().handle();
        let hooks = runtime.engine().epoch_hooks();
        let store = ProgramStore::open(cfg.paths.programs.clone())?;
        let audit = RotatingAudit::open(&cfg.paths.audit)
            .map_err(|e| crate::error::ApiError::internal(format!("audit log: {e}")))?;
        let diag_cursor = ScanDiagCursor::from_status(&runtime.engine().status());
        Ok(Self {
            runtime: Arc::new(Mutex::new(runtime)),
            scan_handle,
            hooks,
            auth: Arc::new(RwLock::new(auth)),
            config: Arc::new(RwLock::new(cfg)),
            config_path: Arc::new(config_path),
            store: Arc::new(store),
            audit: Arc::new(audit),
            events: Arc::new(EventRing::new()),
            diag_cursor: Arc::new(Mutex::new(diag_cursor)),
            upload_sem: Arc::new(Semaphore::new(1)),
            arm_lock: Arc::new(tokio::sync::Mutex::new(())),
            force_limit: Arc::new(Mutex::new(ForceLimiter::new())),
            started: Instant::now(),
            http_ok: Arc::new(AtomicU64::new(0)),
            http_err: Arc::new(AtomicU64::new(0)),
            activate_job: Arc::new(Mutex::new(None)),
        })
    }

    /// Wall-clock unix seconds for audit/events.
    #[must_use]
    pub fn unix_secs(&self) -> u64 {
        SystemClock.unix_secs()
    }

    /// Copy scan edges into the diagnostics ring. Counters stay on the scan thread.
    pub fn poll_scan_diagnostics(&self) {
        let snap = {
            let rt = self.runtime.lock().expect("runtime");
            rt.engine().status()
        };
        let mut cursor = self.diag_cursor.lock().expect("diag cursor");
        let unix = self.unix_secs();
        if snap.mode == OperatingMode::Fault && cursor.mode != OperatingMode::Fault {
            self.events.push(unix, "fault", "mode=FAULT");
        }
        if snap.io_degraded && !cursor.io_degraded {
            self.events.push(unix, "io_degraded", "quality=Bad");
        }
        for task in &snap.tasks {
            let prev = cursor
                .overruns
                .iter()
                .find(|(name, _)| name == &task.name)
                .map_or(0, |(_, count)| *count);
            if task.overruns > prev {
                let delta = task.overruns - prev;
                self.events.push(
                    unix,
                    "logic_overrun",
                    format!("task={} delta={delta}", task.name),
                );
            }
        }
        cursor.mode = snap.mode;
        cursor.io_degraded = snap.io_degraded;
        cursor.overruns = snap
            .tasks
            .iter()
            .map(|task| (task.name.clone(), task.overruns))
            .collect();
    }

    /// Append audit + diagnostics.
    pub fn record(
        &self,
        principal_id: &str,
        action: plc_auth::AuditAction,
        detail: impl Into<String>,
        client_ip: Option<SocketAddr>,
    ) {
        let detail = detail.into();
        let unix = self.unix_secs();
        self.audit.record(AuditEvent {
            unix_secs: unix,
            principal_id: principal_id.to_string(),
            action,
            detail: detail.clone(),
            client_ip: client_ip.map(|a| a.ip()),
        });
        self.events.push(unix, format!("{action:?}"), detail);
    }

    /// Max upload bytes from live config.
    #[must_use]
    pub fn max_package_bytes(&self) -> usize {
        self.config.read().expect("config").limits.max_package_bytes as usize
    }

    /// Snapshot current/armed ids under the runtime mutex, then write pointer files.
    pub fn sync_program_pointers(&self) {
        let (current, armed) = {
            let rt = self.runtime.lock().expect("runtime");
            (
                rt.current_info().map(|p| p.id.clone()),
                rt.armed_info().map(|p| p.id.clone()),
            )
        };
        let _ = self.store.set_pointer("current", current.as_deref());
        let _ = self.store.set_pointer("armed", armed.as_deref());
    }
}

struct ScanDiagCursor {
    mode: OperatingMode,
    io_degraded: bool,
    overruns: Vec<(String, u32)>,
}

impl ScanDiagCursor {
    fn from_status(snap: &ScanStatusSnapshot) -> Self {
        Self {
            mode: snap.mode,
            io_degraded: snap.io_degraded,
            overruns: snap
                .tasks
                .iter()
                .map(|task| (task.name.clone(), task.overruns))
                .collect(),
        }
    }
}
