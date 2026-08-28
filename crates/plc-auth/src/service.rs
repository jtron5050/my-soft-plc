//! [`AuthService`]: authenticate, authorize, lockout, rate limit.

use std::collections::BTreeSet;
use std::fmt;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use plc_config::{AuthConfig, LimitsConfig, PrincipalConfig};

use crate::clock::{Clock, SystemClock};
use crate::credential::{ct_eq, hash_secret, hex_decode_32, Credential};
use crate::dual_control::dual_control_allowed;
use crate::error::AuthError;
use crate::lockout::LockoutTracker;
use crate::principal::{AuthMethod, Principal, ANONYMOUS_ID};
use crate::rate_limit::RateLimiter;
use crate::role::{Permission, Role};

struct StoredPrincipal {
    id: String,
    role: Role,
    token_hash: Option<[u8; 32]>,
    cert_hash: Option<[u8; 32]>,
}

/// Non-RT authn/authz service. `Send + Sync` for later axum `State`.
pub struct AuthService {
    required: bool,
    dual_control: bool,
    principals: Vec<StoredPrincipal>,
    lockout: Mutex<LockoutTracker>,
    rate: Mutex<RateLimiter>,
    clock: Arc<dyn Clock>,
}

impl fmt::Debug for AuthService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthService")
            .field("required", &self.required)
            .field("dual_control", &self.dual_control)
            .field("principals", &self.principals.len())
            .finish_non_exhaustive()
    }
}

impl AuthService {
    /// Build from validated device config, using [`SystemClock`].
    pub fn from_config(auth: &AuthConfig, limits: &LimitsConfig) -> Result<Self, AuthError> {
        Self::from_config_with_clock(auth, limits, Arc::new(SystemClock))
    }

    /// Build with an injected clock (tests).
    pub fn from_config_with_clock(
        auth: &AuthConfig,
        limits: &LimitsConfig,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, AuthError> {
        if auth.lockout_secs == 0 {
            return Err(AuthError::Config("auth.lockout_secs must be >= 1".into()));
        }
        if auth.required && auth.principals.is_empty() {
            return Err(AuthError::Config(
                "auth.required=true requires at least one principal".into(),
            ));
        }

        let mut principals = Vec::with_capacity(auth.principals.len());
        let mut ids = BTreeSet::new();
        let mut token_hashes = BTreeSet::new();
        let mut cert_hashes = BTreeSet::new();
        for p in &auth.principals {
            principals.push(load_principal(
                p,
                &mut ids,
                &mut token_hashes,
                &mut cert_hashes,
            )?);
        }

        Ok(Self {
            required: auth.required,
            dual_control: auth.dual_control,
            principals,
            lockout: Mutex::new(LockoutTracker::new(
                limits.auth_fail_per_min,
                u64::from(auth.lockout_secs),
            )),
            rate: Mutex::new(RateLimiter::new(
                f64::from(limits.rest_rate_per_s),
                f64::from(limits.rest_burst),
            )),
            clock,
        })
    }

    /// Authenticate `cred` from `client_ip`. Records lockout on failure.
    ///
    /// Unknown user and bad token both return [`AuthError::Unauthenticated`].
    /// A locked IP returns [`AuthError::Locked`] even with valid credentials.
    pub fn authenticate(
        &self,
        cred: &Credential,
        client_ip: IpAddr,
    ) -> Result<Principal, AuthError> {
        let now = self.clock.now();
        {
            let mut lockout = self.lockout.lock().expect("lockout mutex");
            if let Some(secs) = lockout.locked_remaining(client_ip, now) {
                return Err(AuthError::Locked {
                    retry_after_secs: secs,
                });
            }
        }

        match self.verify(cred) {
            Ok(principal) => Ok(principal),
            Err(err) => {
                if should_count_failure(cred, self.required) {
                    let mut lockout = self.lockout.lock().expect("lockout mutex");
                    if let Some(secs) = lockout.record_failure(client_ip, now) {
                        return Err(AuthError::Locked {
                            retry_after_secs: secs,
                        });
                    }
                }
                Err(err)
            }
        }
    }

    /// Authorize `principal` for `perm`.
    pub fn authorize(&self, principal: &Principal, perm: Permission) -> Result<(), AuthError> {
        if principal.role.allows(perm) {
            Ok(())
        } else {
            Err(AuthError::Forbidden {
                required: perm,
                need: perm.min_role(),
                role: principal.role,
            })
        }
    }

    /// Authenticate then authorize. Does not consume a rate-limit token.
    pub fn check(
        &self,
        cred: &Credential,
        client_ip: IpAddr,
        perm: Permission,
    ) -> Result<Principal, AuthError> {
        let principal = self.authenticate(cred, client_ip)?;
        self.authorize(&principal, perm)?;
        Ok(principal)
    }

    /// Consume one authenticated request for `principal_id`.
    pub fn rate_check(&self, principal_id: &str) -> Result<(), AuthError> {
        let now = self.clock.now();
        let mut rate = self.rate.lock().expect("rate mutex");
        rate.check(principal_id, now)
            .map_err(|retry_after_secs| AuthError::RateLimited { retry_after_secs })
    }

    /// Dual-control helper: may `activator` activate a package uploaded by `uploader`?
    #[must_use]
    pub fn dual_control_ok(&self, uploader: &str, activator: &str) -> bool {
        dual_control_allowed(self.dual_control, uploader, activator)
    }

    fn verify(&self, cred: &Credential) -> Result<Principal, AuthError> {
        match cred {
            Credential::None => {
                if self.required {
                    Err(AuthError::Unauthenticated)
                } else {
                    Ok(Principal {
                        id: ANONYMOUS_ID.to_string(),
                        role: Role::Admin,
                        method: AuthMethod::Anonymous,
                    })
                }
            }
            Credential::Bearer(token) => {
                let hash = hash_secret(token.as_bytes());
                self.match_token(&hash).ok_or(AuthError::Unauthenticated)
            }
            Credential::ClientCert {
                fingerprint_sha256, ..
            } => self
                .match_cert(fingerprint_sha256)
                .ok_or(AuthError::Unauthenticated),
        }
    }

    fn match_token(&self, hash: &[u8; 32]) -> Option<Principal> {
        let mut found = None;
        for p in &self.principals {
            if let Some(stored) = p.token_hash {
                if ct_eq(&stored, hash) && found.is_none() {
                    found = Some(Principal {
                        id: p.id.clone(),
                        role: p.role,
                        method: AuthMethod::Bearer,
                    });
                }
            }
        }
        found
    }

    fn match_cert(&self, fingerprint: &[u8; 32]) -> Option<Principal> {
        let mut found = None;
        for p in &self.principals {
            if let Some(stored) = p.cert_hash {
                if ct_eq(&stored, fingerprint) && found.is_none() {
                    found = Some(Principal {
                        id: p.id.clone(),
                        role: p.role,
                        method: AuthMethod::Mtls,
                    });
                }
            }
        }
        found
    }
}

fn should_count_failure(cred: &Credential, required: bool) -> bool {
    match cred {
        Credential::None => required,
        Credential::Bearer(_) | Credential::ClientCert { .. } => true,
    }
}

fn load_principal(
    p: &PrincipalConfig,
    ids: &mut BTreeSet<String>,
    token_hashes: &mut BTreeSet<[u8; 32]>,
    cert_hashes: &mut BTreeSet<[u8; 32]>,
) -> Result<StoredPrincipal, AuthError> {
    let id = p.id.trim();
    if id.is_empty() {
        return Err(AuthError::Config("principal id must be non-empty".into()));
    }
    if !ids.insert(id.to_string()) {
        return Err(AuthError::Config(format!("duplicate principal id '{id}'")));
    }
    let role = Role::parse(&p.role)?;
    let token_hash = parse_optional_hash(p.token_sha256.as_deref())?;
    let cert_hash = parse_optional_hash(p.cert_sha256.as_deref())?;
    if token_hash.is_none() && cert_hash.is_none() {
        return Err(AuthError::Config(format!(
            "principal '{id}': at least one of token_sha256 or cert_sha256 is required"
        )));
    }
    if let Some(h) = token_hash {
        if !token_hashes.insert(h) {
            return Err(AuthError::Config(format!(
                "principal '{id}': duplicate token_sha256"
            )));
        }
    }
    if let Some(h) = cert_hash {
        if !cert_hashes.insert(h) {
            return Err(AuthError::Config(format!(
                "principal '{id}': duplicate cert_sha256"
            )));
        }
    }
    Ok(StoredPrincipal {
        id: id.to_string(),
        role,
        token_hash,
        cert_hash,
    })
}

fn parse_optional_hash(s: Option<&str>) -> Result<Option<[u8; 32]>, AuthError> {
    match s.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(h) => Ok(Some(hex_decode_32(h)?)),
    }
}
