//! T0 supervisor: scan thread, REST, MQTT, retain flusher.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use plc_api::{bind_listener, serve_on, AppState};
use plc_config::{DeviceConfig, ProfileKind};
use plc_io::{FieldGate, IoDriver, IoMap};
use plc_io_modbus::{validate_map, ModbusBridge};
use plc_io_sim::SharedSim;
use plc_retain::RetainStore;
use plc_runtime::{spawn_scan_thread, Runtime, RuntimeConfig};
use plc_scan::{ModeRequest, ScanPlan};
use plc_telemetry::TelemetryService;
use plc_types::ProgramPhase;
use tokio::net::TcpListener;

use crate::error::AppError;
use crate::io_route::RoutedDriver;

/// Running process handles.
pub struct Supervisor {
    /// REST + runtime state.
    pub state: AppState,
    /// Bound listen address.
    pub listen: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    listener: Option<TcpListener>,
    scan: Option<std::thread::JoinHandle<()>>,
    tel_handle: Option<plc_telemetry::TelemetryHandle>,
}

impl Supervisor {
    /// Load config, spawn T1, bind REST, optionally load a program.
    pub async fn boot(
        cfg: DeviceConfig,
        config_path: Option<PathBuf>,
        program: Option<PathBuf>,
        mode: Option<&str>,
    ) -> Result<Self, AppError> {
        if cfg.profile == ProfileKind::Dev {
            tracing::warn!(
                "profile=dev allows unsigned packages and plaintext HTTP; not for plant networks"
            );
        }

        fs::create_dir_all(&cfg.paths.programs).map_err(AppError::from)?;
        fs::create_dir_all(&cfg.paths.retain).map_err(AppError::from)?;
        fs::create_dir_all(&cfg.paths.audit).map_err(AppError::from)?;

        let io_map_path = resolve_path(config_path.as_deref(), &cfg.paths.io_map);
        let map = IoMap::load_from_path(&io_map_path)?;
        check_io_drivers(&cfg, &map)?;
        let resolved = map.resolve()?;
        let plan = validate_map(&map, &resolved)?;
        let image = resolved.image.clone();
        let n_i = image.inputs.len();
        let n_q = image.outputs.len();
        let sim = SharedSim::new("sim", n_i, n_q);
        let (driver, field_gate): (Box<dyn IoDriver>, Option<Arc<FieldGate>>) =
            if plan.modules.is_empty() {
                (Box::new(sim.clone()), None)
            } else {
                let gate = Arc::new(FieldGate::new());
                let bridge = ModbusBridge::new(plan, Arc::clone(&gate));
                (Box::new(RoutedDriver::new(sim.clone(), bridge)), Some(gate))
            };
        let mut io = plc_scan::ScanIo::new(image, driver);
        io.field_gate = field_gate;
        let plan = ScanPlan::from_config(&cfg).map_err(|e| AppError::config(e.to_string()))?;
        let mut rt = Runtime::new(
            plan,
            io,
            Box::new(plc_scan::MonotonicClock::new()),
            RuntimeConfig {
                require_signature: cfg.program.require_signature,
                ..RuntimeConfig::default()
            },
        )?;
        rt.set_input_injector(Arc::new(sim));

        let tel_src = rt.engine().telemetry_source();
        let scan_handle = rt.engine().handle();
        let retain_watch = rt.engine().retain_dirty();
        let affinity = cfg.scan.cpu_affinity;

        let mut tel = if cfg.telemetry.enabled {
            match TelemetryService::from_config(&cfg, tel_src, scan_handle.clone()) {
                Ok(t) => Some(t),
                Err(e) => {
                    tracing::warn!("telemetry disabled: {e}");
                    None
                }
            }
        } else {
            None
        };
        let tel_handle = tel.as_ref().map(TelemetryService::handle);

        let state = AppState::new(cfg.clone(), rt, config_path)?;
        let stop = Arc::new(AtomicBool::new(false));
        // Bind before T1 so a listen failure cannot leak the scan thread.
        let listener = bind_listener(&state).await?;
        let listen = listener.local_addr().map_err(AppError::from)?;
        tracing::info!("REST listening on {listen}");
        let scan = spawn_scan_thread(state.runtime.clone(), stop.clone(), affinity);

        if let Some(svc) = tel.take() {
            tokio::spawn(async move {
                if let Err(e) = svc.run().await {
                    tracing::warn!("telemetry worker ended: {e}");
                }
            });
        }

        let retain_dir = cfg.paths.retain.clone();
        let retain_state = state.clone();
        let retain_stop = stop.clone();
        tokio::spawn(async move {
            retain_flusher(retain_dir, retain_state, retain_watch, retain_stop).await;
        });

        let follow_state = state.clone();
        let follow_tel = tel_handle.clone();
        let follow_stop = stop.clone();
        tokio::spawn(async move {
            follow_current_program(follow_state, follow_tel, follow_stop).await;
        });

        let sup = Self {
            state,
            listen,
            stop,
            listener: Some(listener),
            scan: Some(scan),
            tel_handle,
        };
        sd_notify("READY=1");

        if let Some(path) = program {
            sup.load_program_file(&path).await?;
        } else {
            sup.restore_current().await?;
        }
        if let Some(mode) = mode {
            sup.set_mode(mode)?;
        }
        Ok(sup)
    }

    /// Serve REST until cancelled (ctrl-c / [`Self::shutdown`]).
    pub async fn serve(mut self) -> Result<(), AppError> {
        let listener = self
            .listener
            .take()
            .ok_or_else(|| AppError::runtime("listener already taken"))?;
        let state = self.state.clone();
        let result = tokio::select! {
            r = serve_on(listener, state) => r.map_err(AppError::from),
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("shutdown signal");
                Ok(())
            }
        };
        self.shutdown();
        result
    }

    /// Request STOP and join the scan thread.
    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.state.scan_handle.request_mode(ModeRequest::Stop);
        if let Some(h) = self.scan.take() {
            let _ = h.join();
        }
        flush_retain_once(&self.state);
        sd_notify("STOPPING=1");
    }

    async fn load_program_file(&self, path: &Path) -> Result<(), AppError> {
        let bytes = std::fs::read(path)?;
        self.install_package(&bytes).await
    }

    async fn restore_current(&self) -> Result<(), AppError> {
        let Some(id) = self.state.store.pointer("current") else {
            return Ok(());
        };
        let (_meta, bytes) = self
            .state
            .store
            .get(&id)
            .map_err(|e| AppError::runtime(format!("{e:?}")))?;
        self.install_package(&bytes).await
    }

    async fn install_package(&self, bytes: &[u8]) -> Result<(), AppError> {
        self.state
            .store
            .put(bytes, Some("boot"), unix_secs())
            .map_err(|e| AppError::runtime(format!("{e:?}")))?;
        {
            let mut rt = self.state.runtime.lock().expect("runtime");
            rt.upload(bytes)?;
            rt.activate()?;
        }
        wait_current(&self.state).await?;
        self.state.sync_program_pointers();
        let id = {
            let rt = self.state.runtime.lock().expect("runtime");
            rt.current_info().map(|p| p.id.clone())
        };
        if let Some(id) = id {
            self.restore_retain(&id);
        }
        self.push_catalog();
        Ok(())
    }

    fn restore_retain(&self, program_id: &str) {
        let store = match RetainStore::open(
            self.state
                .config
                .read()
                .expect("config")
                .paths
                .retain
                .clone(),
        ) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("retain store: {e}");
                return;
            }
        };
        let mut rt = self.state.runtime.lock().expect("runtime");
        let Some(layout) = rt.current_retain_layout().cloned() else {
            return;
        };
        let mut buf = vec![0u8; layout.retain_size as usize];
        match store.load(program_id, &layout, &mut buf) {
            Ok(report) => {
                if let Err(e) = rt.load_retain_image(&buf) {
                    tracing::warn!("retain apply: {e}");
                } else {
                    if let (Some(snap), Some(bytes)) =
                        (rt.retain_snapshot(), rt.current_retain_bytes())
                    {
                        snap.publish(bytes);
                    }
                    tracing::info!(
                        "retain restored source={:?} kept={}",
                        report.source,
                        report.kept
                    );
                }
            }
            Err(e) => tracing::warn!("retain load: {e}"),
        }
    }

    fn push_catalog(&self) {
        let Some(handle) = &self.tel_handle else {
            return;
        };
        let rt = self.state.runtime.lock().expect("runtime");
        match rt.telemetry_catalog() {
            Ok(cat) => handle.set_catalog(cat),
            Err(e) => tracing::warn!("telemetry catalog: {e}"),
        }
    }

    fn set_mode(&self, mode: &str) -> Result<(), AppError> {
        let req = match mode.to_ascii_uppercase().as_str() {
            "STOP" => ModeRequest::Stop,
            "SIM" => ModeRequest::Sim,
            "RUN" => ModeRequest::Run,
            other => return Err(AppError::config(format!("unknown --mode {other}"))),
        };
        self.state.scan_handle.request_mode(req);
        Ok(())
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        if !self.stop.load(Ordering::Relaxed) {
            self.shutdown();
        }
    }
}

fn check_io_drivers(cfg: &DeviceConfig, map: &IoMap) -> Result<(), AppError> {
    for driver in &cfg.io.drivers {
        if driver == "gpio" {
            return Err(AppError::config(
                "gpio is PR-17 (this runtime accepts io.drivers sim and modbus_tcp)",
            ));
        }
        if driver != "sim" && driver != "modbus_tcp" {
            return Err(AppError::config(format!(
                "unsupported io.drivers entry '{driver}'"
            )));
        }
    }
    let enabled: BTreeSet<&str> = cfg.io.drivers.iter().map(String::as_str).collect();
    let mut modbus_modules = 0usize;
    for module in &map.modules {
        if !enabled.contains(module.driver.as_str()) {
            return Err(AppError::config(format!(
                "io-map module '{}' uses driver '{}' which is not listed in io.drivers",
                module.id, module.driver
            )));
        }
        if module.driver == "modbus_tcp" {
            modbus_modules += 1;
        }
    }
    if enabled.contains("modbus_tcp") && modbus_modules == 0 {
        return Err(AppError::config(
            "io.drivers includes modbus_tcp but the io-map has no modbus_tcp module",
        ));
    }
    Ok(())
}

fn resolve_path(config_path: Option<&Path>, p: &str) -> PathBuf {
    let path = Path::new(p);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    if path.exists() {
        return path.to_path_buf();
    }
    if let Some(dir) = config_path.and_then(|c| c.parent()) {
        let joined = dir.join(path);
        if joined.exists() {
            return joined;
        }
        if let Some(name) = path.file_name() {
            let next_to_cfg = dir.join(name);
            if next_to_cfg.exists() {
                return next_to_cfg;
            }
        }
    }
    path.to_path_buf()
}

/// REST activate never calls the boot install path; keep pointers + catalog
/// in sync whenever `current_info` settles after an epoch swap.
async fn follow_current_program(
    state: AppState,
    tel: Option<plc_telemetry::TelemetryHandle>,
    stop: Arc<AtomicBool>,
) {
    let mut last: Option<(String, String)> = None;
    while !stop.load(Ordering::Relaxed) {
        let key = settled_current_key(&state);
        if key != last {
            last.clone_from(&key);
            state.sync_program_pointers();
            if key.is_some() {
                if let Some(handle) = &tel {
                    let cat = {
                        let rt = state.runtime.lock().expect("runtime");
                        rt.telemetry_catalog()
                    };
                    match cat {
                        Ok(cat) => handle.set_catalog(cat),
                        Err(e) => tracing::warn!("telemetry catalog: {e}"),
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn settled_current_key(state: &AppState) -> Option<(String, String)> {
    let rt = state.runtime.lock().expect("runtime");
    if rt.phase() != ProgramPhase::Idle {
        return None;
    }
    rt.current_info()
        .map(|p| (p.id.clone(), p.compatibility_hash.clone()))
}

async fn wait_current(state: &AppState) -> Result<(), AppError> {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        {
            let rt = state.runtime.lock().expect("runtime");
            if rt.current_info().is_some() && rt.phase() == ProgramPhase::Idle {
                return Ok(());
            }
        }
        if std::time::Instant::now() > deadline {
            return Err(AppError::runtime("timed out waiting for program activate"));
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

async fn retain_flusher(
    dir: String,
    state: AppState,
    watch: plc_scan::RetainDirtyWatch,
    stop: Arc<AtomicBool>,
) {
    let store = match RetainStore::open(&dir) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("retain flusher disabled: {e}");
            return;
        }
    };
    while !stop.load(Ordering::Relaxed) {
        if watch.take().is_some() {
            flush_retain(&store, &state);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    flush_retain(&store, &state);
}

fn flush_retain_once(state: &AppState) {
    let dir = state.config.read().expect("config").paths.retain.clone();
    if let Ok(store) = RetainStore::open(dir) {
        flush_retain(&store, state);
    }
}

fn flush_retain(store: &RetainStore, state: &AppState) {
    let snapshot = (|| {
        let rt = state.runtime.lock().expect("runtime");
        let info = rt.current_info()?;
        let layout = rt.current_retain_layout()?.clone();
        let buf = rt.retain_snapshot()?;
        let mut image = vec![0u8; layout.retain_size as usize];
        buf.read(&mut image)?;
        Some((info.id.clone(), layout, image))
    })();
    let Some((id, layout, image)) = snapshot else {
        return;
    };
    if let Err(e) = store.flush(&id, &layout, &image) {
        tracing::warn!("retain flush: {e}");
    }
}

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn sd_notify(msg: &str) {
    let Ok(path) = std::env::var("NOTIFY_SOCKET") else {
        return;
    };
    #[cfg(unix)]
    {
        let _ = std::os::unix::net::UnixDatagram::unbound()
            .and_then(|s| s.send_to(msg.as_bytes(), path));
    }
    #[cfg(not(unix))]
    {
        let _ = (msg, path);
    }
}
