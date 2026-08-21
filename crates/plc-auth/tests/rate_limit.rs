//! Authenticated request token bucket.

mod common;

use std::sync::Arc;
use std::time::Duration;

use plc_auth::{AuthError, AuthService, FakeClock};

use common::{required_auth, tight_limits};

#[test]
fn burst_then_limited_then_refill() {
    let clock = Arc::new(FakeClock::new());
    let svc = AuthService::from_config_with_clock(&required_auth(), &tight_limits(), clock.clone())
        .unwrap();

    svc.rate_check("eng1").unwrap();
    svc.rate_check("eng1").unwrap();
    let err = svc.rate_check("eng1").unwrap_err();
    match err {
        AuthError::RateLimited { retry_after_secs } => {
            assert!(retry_after_secs >= 1);
            assert_eq!(err.http_status(), 429);
        }
        other => panic!("expected rate limited, got {other}"),
    }

    clock.advance(Duration::from_secs(1));
    svc.rate_check("eng1").unwrap();
}

#[test]
fn buckets_are_per_principal() {
    let clock = Arc::new(FakeClock::new());
    let svc =
        AuthService::from_config_with_clock(&required_auth(), &tight_limits(), clock).unwrap();
    svc.rate_check("eng1").unwrap();
    svc.rate_check("eng1").unwrap();
    assert!(svc.rate_check("eng1").is_err());
    svc.rate_check("op1").unwrap();
}

#[test]
fn architecture_defaults_burst_60() {
    let clock = Arc::new(FakeClock::new());
    let svc =
        AuthService::from_config_with_clock(&required_auth(), &common::limits(), clock.clone())
            .unwrap();
    for i in 0..60 {
        svc.rate_check("eng1")
            .unwrap_or_else(|e| panic!("request {i} should succeed: {e}"));
    }
    assert!(matches!(
        svc.rate_check("eng1"),
        Err(AuthError::RateLimited { .. })
    ));
    clock.advance(Duration::from_secs(1));
    svc.rate_check("eng1").unwrap();
}
