//! Bearer / mTLS credential extraction and permission checks.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::extract::connect_info::ConnectInfo;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{header, HeaderMap};
use plc_auth::{AuthService, Credential, Permission, Principal};

use crate::error::ApiError;
use crate::state::AppState;

/// SHA-256 of client certificate DER, inserted by the TLS acceptor.
#[derive(Debug, Clone, Copy)]
pub struct ClientCertFp(pub [u8; 32]);

/// Authenticated caller (rate-limited). Authorize per-handler.
#[derive(Debug, Clone)]
pub struct Authed {
    /// Principal.
    pub principal: Principal,
    /// Peer address if known.
    pub addr: SocketAddr,
}

impl Authed {
    /// Authorize `perm`.
    pub fn require(&self, state: &AppState, perm: Permission) -> Result<(), ApiError> {
        let auth = state.auth.read().expect("auth");
        auth.authorize(&self.principal, perm)?;
        Ok(())
    }
}

impl FromRequestParts<AppState> for Authed {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let addr = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map_or(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0), |c| c.0);
        let fp = parts.extensions.get::<ClientCertFp>().copied();
        let cred = credential_from(&parts.headers, fp);
        let auth = state.auth.read().expect("auth");
        let principal = authenticate_and_rate(&auth, &cred, addr.ip(), state, addr)?;
        Ok(Self { principal, addr })
    }
}

fn authenticate_and_rate(
    auth: &AuthService,
    cred: &Credential,
    ip: IpAddr,
    state: &AppState,
    addr: SocketAddr,
) -> Result<Principal, ApiError> {
    match auth.authenticate(cred, ip) {
        Ok(p) => {
            if let Err(e) = auth.rate_check(&p.id) {
                return Err(e.into());
            }
            Ok(p)
        }
        Err(e) => {
            let action = match &e {
                plc_auth::AuthError::Locked { .. } => plc_auth::AuditAction::AuthLocked,
                _ => plc_auth::AuditAction::AuthFailure,
            };
            state.record("anonymous", action, e.to_string(), Some(addr));
            Err(e.into())
        }
    }
}

fn credential_from(headers: &HeaderMap, fp: Option<ClientCertFp>) -> Credential {
    if let Some(value) = headers.get(header::AUTHORIZATION) {
        if let Ok(s) = value.to_str() {
            if let Some(token) = s.strip_prefix("Bearer ") {
                let token = token.trim();
                if !token.is_empty() {
                    return Credential::Bearer(token.to_string());
                }
            }
        }
    }
    if let Some(ClientCertFp(fingerprint_sha256)) = fp {
        return Credential::ClientCert {
            fingerprint_sha256,
            subject_cn: None,
        };
    }
    Credential::None
}
