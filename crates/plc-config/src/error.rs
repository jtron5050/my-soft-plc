//! Configuration load and validation errors.

use std::path::PathBuf;

use thiserror::Error;

/// Errors produced while loading or validating device configuration.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Filesystem I/O failure.
    #[error("config I/O error at {path}: {source}")]
    Io {
        /// Path involved in the failure (best effort).
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// YAML parse failure.
    #[error("YAML parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    /// JSON parse failure.
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
    /// Schema version unsupported or missing.
    #[error("unsupported config schema version: {0} (expected {1})")]
    UnsupportedVersion(u32, u32),
    /// Semantic validation failure (stable message for golden tests).
    #[error("config validation failed: {0}")]
    Validation(String),
}

impl ConfigError {
    /// Build a validation error with a stable message string.
    #[must_use]
    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Validation(msg.into())
    }
}
