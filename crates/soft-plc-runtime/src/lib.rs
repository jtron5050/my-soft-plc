//! Process supervisor for `soft-plc-runtime` (architecture PR-14).

#![forbid(unsafe_code)]

mod cli;
mod error;
mod supervisor;

pub use cli::Args;
pub use error::AppError;
pub use supervisor::Supervisor;

use std::path::PathBuf;

use plc_config::{load_from_path, DeviceConfig};

/// Apply CLI overlays (`--data-dir`, `--bind`) to a loaded config.
pub fn apply_cli(cfg: &mut DeviceConfig, data_dir: Option<&PathBuf>, bind: Option<&str>) {
    if let Some(dir) = data_dir {
        cfg.paths.programs = dir.join("programs").to_string_lossy().into_owned();
        cfg.paths.retain = dir.join("retain").to_string_lossy().into_owned();
        cfg.paths.audit = dir.join("audit").to_string_lossy().into_owned();
    }
    if let Some(bind) = bind {
        cfg.rest.bind = bind.to_string();
    }
}

/// Load device config from `path`.
pub fn load_config(path: &std::path::Path) -> Result<DeviceConfig, AppError> {
    Ok(load_from_path(path)?)
}
