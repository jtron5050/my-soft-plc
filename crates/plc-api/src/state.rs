//! Shared axum state.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use plc_auth::{AuditEvent, AuditSink, AuthService, Clock, MemoryAudit, SystemClock};
use plc_config::DeviceConfig;
use plc_runtime::Runtime;
use plc_scan::{EpochHooks, ScanHandle};
use tokio::sync::Semaphore;

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
    /// Audit ring.
    pub audit: Arc<MemoryAudit>,
    /// Diagnostics ring.
    pub events: Arc<EventRing>,
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
        Ok(Self {
            runtime: Arc::new(Mutex::new(runtime)),
            scan_handle,
            hooks,
            auth: Arc::new(RwLock::new(auth)),
            config: Arc::new(RwLock::new(cfg)),
            config_path: Arc::new(config_path),
            store: Arc::new(store),
            audit: Arc::new(MemoryAudit::new()),
            events: Arc::new(EventRing::new()),
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
}
