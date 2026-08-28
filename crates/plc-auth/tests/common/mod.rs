//! Shared builders for `plc-auth` integration tests.

#![allow(dead_code)]

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use plc_auth::{hash_secret, hex_encode, AuthService, FakeClock};
use plc_config::{AuthConfig, LimitsConfig, PrincipalConfig};

pub const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
pub const OTHER_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
pub const ENG_SECRET: &str = "test-token-engineer";
pub const OP_SECRET: &str = "test-token-operator";
pub const VIEW_SECRET: &str = "test-token-viewer";
pub const ADMIN_SECRET: &str = "test-token-admin";
pub const CERT_DER: &[u8] = b"fake-client-cert-der";

pub fn token_hex(secret: &str) -> String {
    hex_encode(&hash_secret(secret.as_bytes()))
}

pub fn cert_hex() -> String {
    hex_encode(&hash_secret(CERT_DER))
}

pub fn principal(id: &str, role: &str, token: Option<&str>, cert: Option<&str>) -> PrincipalConfig {
    PrincipalConfig {
        id: id.into(),
        role: role.into(),
        token_sha256: token.map(str::to_string),
        cert_sha256: cert.map(str::to_string),
    }
}

pub fn limits() -> LimitsConfig {
    LimitsConfig::default()
}

pub fn tight_limits() -> LimitsConfig {
    LimitsConfig {
        rest_rate_per_s: 1,
        rest_burst: 2,
        auth_fail_per_min: 5,
        ..LimitsConfig::default()
    }
}

pub fn required_auth() -> AuthConfig {
    AuthConfig {
        required: true,
        principals: vec![
            principal("viewer1", "viewer", Some(&token_hex(VIEW_SECRET)), None),
            principal("op1", "operator", Some(&token_hex(OP_SECRET)), None),
            principal("eng1", "engineer", Some(&token_hex(ENG_SECRET)), None),
            principal("adm1", "admin", Some(&token_hex(ADMIN_SECRET)), None),
            principal("mtls1", "engineer", None, Some(&cert_hex())),
        ],
        ..AuthConfig::default()
    }
}

pub fn service_required() -> (AuthService, Arc<FakeClock>) {
    let clock = Arc::new(FakeClock::new());
    let svc = AuthService::from_config_with_clock(&required_auth(), &limits(), clock.clone())
        .expect("required auth config");
    (svc, clock)
}

pub fn service_optional() -> AuthService {
    let auth = AuthConfig::default();
    AuthService::from_config(&auth, &limits()).expect("optional auth")
}
