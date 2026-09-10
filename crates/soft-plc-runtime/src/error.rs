//! Process-level errors.

use thiserror::Error;

/// Supervisor / CLI failures (never FAULT on their own).
#[derive(Debug, Error)]
pub enum AppError {
    /// Config or io-map.
    #[error("{0}")]
    Config(String),
    /// I/O, package, runtime, API.
    #[error("{0}")]
    Runtime(String),
}

impl AppError {
    pub(crate) fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    pub(crate) fn runtime(msg: impl Into<String>) -> Self {
        Self::Runtime(msg.into())
    }
}

impl From<plc_config::ConfigError> for AppError {
    fn from(value: plc_config::ConfigError) -> Self {
        Self::Config(value.to_string())
    }
}

impl From<plc_io::IoError> for AppError {
    fn from(value: plc_io::IoError) -> Self {
        Self::Config(value.to_string())
    }
}

impl From<plc_runtime::RuntimeError> for AppError {
    fn from(value: plc_runtime::RuntimeError) -> Self {
        Self::Runtime(value.to_string())
    }
}

impl From<plc_api::ApiError> for AppError {
    fn from(value: plc_api::ApiError) -> Self {
        Self::Runtime(format!("{value:?}"))
    }
}

impl From<plc_telemetry::TelemetryError> for AppError {
    fn from(value: plc_telemetry::TelemetryError) -> Self {
        Self::Runtime(value.to_string())
    }
}

impl From<plc_retain::RetainError> for AppError {
    fn from(value: plc_retain::RetainError) -> Self {
        Self::Runtime(value.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        Self::Runtime(value.to_string())
    }
}
