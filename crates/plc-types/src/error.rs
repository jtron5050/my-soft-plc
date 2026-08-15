//! Shared error kinds used across crates before richer domain errors exist.

use core::fmt;

/// Result alias for soft-PLC operations that use [`PlcError`].
pub type PlcResult<T> = Result<T, PlcError>;

/// Coarse, crate-agnostic error categories.
///
/// Domain crates may wrap or map into these for REST / diagnostics surfaces.
/// Prefer structured variants over free-form strings at call sites that cross
/// crate boundaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlcError {
    /// Configuration refused or failed validation.
    Config(String),
    /// Program package rejected (signature, IR, size, schema).
    Package(String),
    /// I/O driver or mapper failure (not necessarily FAULT mode).
    Io(String),
    /// Scan / mode transition refused or failed.
    Scan(String),
    /// Authentication or authorization failure.
    Auth(String),
    /// Internal invariant broken or unexpected state.
    Internal(String),
    /// Operation not valid in the current mode or program phase.
    InvalidState {
        /// Human-readable context (e.g. "activate while not armed").
        context: String,
    },
}

impl PlcError {
    /// Stable category name for metrics / logs.
    #[must_use]
    pub const fn category(&self) -> &'static str {
        match self {
            Self::Config(_) => "config",
            Self::Package(_) => "package",
            Self::Io(_) => "io",
            Self::Scan(_) => "scan",
            Self::Auth(_) => "auth",
            Self::Internal(_) => "internal",
            Self::InvalidState { .. } => "invalid_state",
        }
    }
}

impl fmt::Display for PlcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(msg) => write!(f, "config error: {msg}"),
            Self::Package(msg) => write!(f, "package error: {msg}"),
            Self::Io(msg) => write!(f, "io error: {msg}"),
            Self::Scan(msg) => write!(f, "scan error: {msg}"),
            Self::Auth(msg) => write!(f, "auth error: {msg}"),
            Self::Internal(msg) => write!(f, "internal error: {msg}"),
            Self::InvalidState { context } => write!(f, "invalid state: {context}"),
        }
    }
}

impl std::error::Error for PlcError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_and_category() {
        let err = PlcError::InvalidState {
            context: "activate while not armed".into(),
        };
        assert_eq!(err.category(), "invalid_state");
        assert!(err.to_string().contains("activate while not armed"));
    }
}
