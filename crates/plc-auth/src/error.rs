//! Structured authn/authz errors.

use thiserror::Error;

use crate::role::{Permission, Role};
use plc_types::PlcError;

/// Authentication or authorization failure.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// Missing or invalid credentials (HTTP 401).
    #[error("unauthenticated")]
    Unauthenticated,
    /// Authenticated but the role does not grant `required` (HTTP 403).
    #[error("forbidden: permission {required} requires role {need}, have {role}")]
    Forbidden {
        /// Permission that was requested.
        required: Permission,
        /// Minimum role that grants `required`.
        need: Role,
        /// Caller's role.
        role: Role,
    },
    /// Source IP is locked out after too many failures (HTTP 429).
    #[error("locked; retry after {retry_after_secs}s")]
    Locked {
        /// Seconds remaining on the lockout.
        retry_after_secs: u64,
    },
    /// Authenticated request rate exceeded (HTTP 429).
    #[error("rate limited; retry after {retry_after_secs}s")]
    RateLimited {
        /// Seconds until a token is likely available.
        retry_after_secs: u64,
    },
    /// Principal table or config refused.
    #[error("auth config: {0}")]
    Config(String),
}

impl AuthError {
    /// HTTP status hint for PR-12. No `http` crate dependency.
    #[must_use]
    pub const fn http_status(&self) -> u16 {
        match self {
            Self::Unauthenticated => 401,
            Self::Forbidden { .. } => 403,
            Self::Locked { .. } | Self::RateLimited { .. } => 429,
            Self::Config(_) => 500,
        }
    }
}

impl From<AuthError> for PlcError {
    fn from(err: AuthError) -> Self {
        Self::Auth(err.to_string())
    }
}
