//! Per-IP failure window and lockout expiry.

mod common;

use std::time::Duration;

use plc_auth::{AuthError, Credential};

use common::{service_required, ENG_SECRET, LOOPBACK, OTHER_IP};

#[test]
fn five_failures_lock_one_ip() {
    let (svc, _) = service_required();
    for i in 0..4 {
        let err = svc
            .authenticate(&Credential::Bearer("bad".into()), LOOPBACK)
            .unwrap_err();
        assert_eq!(err, AuthError::Unauthenticated, "failure {i}");
    }
    let err = svc
        .authenticate(&Credential::Bearer("bad".into()), LOOPBACK)
        .unwrap_err();
    match err {
        AuthError::Locked { retry_after_secs } => {
            assert!(retry_after_secs >= 1);
            assert_eq!(err.http_status(), 429);
        }
        other => panic!("expected lockout on 5th failure, got {other}"),
    }
}

#[test]
fn other_ip_unaffected() {
    let (svc, _) = service_required();
    for _ in 0..5 {
        let _ = svc.authenticate(&Credential::Bearer("bad".into()), LOOPBACK);
    }
    let p = svc
        .authenticate(&Credential::Bearer(ENG_SECRET.into()), OTHER_IP)
        .unwrap();
    assert_eq!(p.id, "eng1");
}

#[test]
fn valid_token_still_locked() {
    let (svc, _) = service_required();
    for _ in 0..5 {
        let _ = svc.authenticate(&Credential::Bearer("bad".into()), LOOPBACK);
    }
    let err = svc
        .authenticate(&Credential::Bearer(ENG_SECRET.into()), LOOPBACK)
        .unwrap_err();
    assert!(matches!(err, AuthError::Locked { .. }));
}

#[test]
fn lockout_expires_after_lockout_secs() {
    let (svc, clock) = service_required();
    for _ in 0..5 {
        let _ = svc.authenticate(&Credential::Bearer("bad".into()), LOOPBACK);
    }
    assert!(matches!(
        svc.authenticate(&Credential::Bearer(ENG_SECRET.into()), LOOPBACK),
        Err(AuthError::Locked { .. })
    ));

    clock.advance(Duration::from_secs(60));
    let p = svc
        .authenticate(&Credential::Bearer(ENG_SECRET.into()), LOOPBACK)
        .unwrap();
    assert_eq!(p.id, "eng1");
}

#[test]
fn missing_creds_when_required_count_toward_lockout() {
    let (svc, _) = service_required();
    for _ in 0..5 {
        let _ = svc.authenticate(&Credential::None, LOOPBACK);
    }
    assert!(matches!(
        svc.authenticate(&Credential::None, LOOPBACK),
        Err(AuthError::Locked { .. })
    ));
}
