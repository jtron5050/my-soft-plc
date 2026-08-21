//! Authenticated identity.

use crate::role::Role;

/// Principal id used when `auth.required=false` and no credentials are presented.
pub const ANONYMOUS_ID: &str = "anonymous";

/// How a principal was authenticated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    /// `Authorization: Bearer` opaque token.
    Bearer,
    /// Client certificate fingerprint match.
    Mtls,
    /// `auth.required=false` and `Credential::None`.
    Anonymous,
}

/// An authenticated (or anonymous-admin) actor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    /// Configured principal id, or [`ANONYMOUS_ID`].
    pub id: String,
    /// Granted role.
    pub role: Role,
    /// How identity was established.
    pub method: AuthMethod,
}
