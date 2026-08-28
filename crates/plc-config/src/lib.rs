//! Versioned device configuration for the soft PLC runtime.
//!
//! Loads YAML or JSON from disk, validates limits and cross-field rules, and
//! exposes a stable schema for REST and scan setup (architecture PR-02).

#![forbid(unsafe_code)]

mod error;
mod schema;
mod store;
mod validate;

pub use error::ConfigError;
pub use schema::{
    AuthConfig, DeviceConfig, DeviceIdentity, IoConfig, LimitsConfig, PathsConfig, PrincipalConfig,
    ProfileKind, ProgramConfig, ScanConfig, StopOutputPolicy, TaskConfig, TelemetryConfig,
    WatchdogConfig, AUTH_ROLES, CONFIG_SCHEMA_VERSION,
};
pub use store::{load_from_path, load_from_reader, load_from_str, save_to_path, ConfigFormat};
pub use validate::validate;
