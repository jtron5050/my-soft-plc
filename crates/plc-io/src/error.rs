//! I/O subsystem errors.

use thiserror::Error;

/// Errors from drivers, mapper, or image operations.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum IoError {
    /// Driver failed to start or is not running.
    #[error("driver not ready: {0}")]
    NotReady(String),
    /// Poll or apply failed (quality may also be set Bad).
    #[error("driver I/O failure: {0}")]
    Driver(String),
    /// Binding / image index out of range.
    #[error("image bounds: {0}")]
    Bounds(String),
    /// Invalid map configuration.
    #[error("io-map error: {0}")]
    Map(String),
}
