//! Authentication and authorization primitives for the management plane.
//!
//! Non-RT. Not used from the scan thread. Does not terminate TLS or serve HTTP;
//! identity is a SHA-256 bearer hash or client-cert fingerprint, never the
//! secret itself.

#![forbid(unsafe_code)]

mod audit;
mod clock;
mod credential;
mod dual_control;
mod error;
mod lockout;
mod principal;
mod rate_limit;
mod role;
mod service;

pub use audit::{AuditAction, AuditEvent, AuditSink, MemoryAudit, AUDIT_CAP};
pub use clock::{Clock, FakeClock, SystemClock};
pub use credential::{hash_secret, hex_decode_32, hex_encode, Credential};
pub use dual_control::dual_control_allowed;
pub use error::AuthError;
pub use principal::{AuthMethod, Principal, ANONYMOUS_ID};
pub use role::{Permission, Role};
pub use service::AuthService;
