//! Bearer, mTLS, anonymous, and required-auth behaviour.

mod common;

use plc_auth::{
    hash_secret, AuthError, AuthMethod, Credential, Permission, Principal, Role, ANONYMOUS_ID,
};
use plc_types::PlcError;

use common::{service_optional, service_required, CERT_DER, ENG_SECRET, LOOPBACK, VIEW_SECRET};

#[test]
fn bearer_happy_path() {
    let (svc, _) = service_required();
    let p = svc
        .authenticate(&Credential::Bearer(ENG_SECRET.into()), LOOPBACK)
        .unwrap();
    assert_eq!(p.id, "eng1");
    assert_eq!(p.role, Role::Engineer);
    assert_eq!(p.method, AuthMethod::Bearer);
}

#[test]
fn unknown_token_is_unauthenticated() {
    let (svc, _) = service_required();
    let err = svc
        .authenticate(&Credential::Bearer("nope".into()), LOOPBACK)
        .unwrap_err();
    assert_eq!(err, AuthError::Unauthenticated);
    assert_eq!(err.http_status(), 401);
}

#[test]
fn required_none_is_unauthenticated() {
    let (svc, _) = service_required();
    let err = svc.authenticate(&Credential::None, LOOPBACK).unwrap_err();
    assert_eq!(err, AuthError::Unauthenticated);
}

#[test]
fn optional_none_is_anonymous_admin() {
    let svc = service_optional();
    let p = svc.authenticate(&Credential::None, LOOPBACK).unwrap();
    assert_eq!(p.id, ANONYMOUS_ID);
    assert_eq!(p.role, Role::Admin);
    assert_eq!(p.method, AuthMethod::Anonymous);
    svc.authorize(&p, Permission::ProgramActivate).unwrap();
    svc.authorize(&p, Permission::UserAdmin).unwrap();
}

#[test]
fn optional_bad_token_still_fails() {
    let svc = service_optional();
    let err = svc
        .authenticate(&Credential::Bearer("nope".into()), LOOPBACK)
        .unwrap_err();
    assert_eq!(err, AuthError::Unauthenticated);
}

#[test]
fn mtls_fingerprint_match() {
    let (svc, _) = service_required();
    let cred = Credential::ClientCert {
        fingerprint_sha256: hash_secret(CERT_DER),
        subject_cn: Some("spoofed".into()),
    };
    let p = svc.authenticate(&cred, LOOPBACK).unwrap();
    assert_eq!(p.id, "mtls1");
    assert_eq!(p.method, AuthMethod::Mtls);
}

#[test]
fn mtls_unknown_fingerprint() {
    let (svc, _) = service_required();
    let cred = Credential::ClientCert {
        fingerprint_sha256: hash_secret(b"other-cert"),
        subject_cn: Some("eng1".into()),
    };
    let err = svc.authenticate(&cred, LOOPBACK).unwrap_err();
    assert_eq!(err, AuthError::Unauthenticated);
}

#[test]
fn cn_is_not_an_auth_factor() {
    let (svc, _) = service_required();
    let cred = Credential::ClientCert {
        fingerprint_sha256: [0u8; 32],
        subject_cn: Some("eng1".into()),
    };
    assert_eq!(
        svc.authenticate(&cred, LOOPBACK).unwrap_err(),
        AuthError::Unauthenticated
    );
}

#[test]
fn check_combines_authn_and_authz() {
    let (svc, _) = service_required();
    let p = svc
        .check(
            &Credential::Bearer(VIEW_SECRET.into()),
            LOOPBACK,
            Permission::StatusRead,
        )
        .unwrap();
    assert_eq!(p.role, Role::Viewer);

    let err = svc
        .check(
            &Credential::Bearer(VIEW_SECRET.into()),
            LOOPBACK,
            Permission::ProgramActivate,
        )
        .unwrap_err();
    match err {
        AuthError::Forbidden {
            required,
            need,
            role,
        } => {
            assert_eq!(required, Permission::ProgramActivate);
            assert_eq!(need, Role::Engineer);
            assert_eq!(role, Role::Viewer);
            assert_eq!(err.http_status(), 403);
        }
        other => panic!("expected forbidden, got {other}"),
    }
}

#[test]
fn auth_error_maps_to_plc_error() {
    let err: PlcError = AuthError::Unauthenticated.into();
    assert_eq!(err.category(), "auth");
}

#[test]
fn authorize_viewer_denied_user_admin() {
    let (svc, _) = service_required();
    let p = Principal {
        id: "viewer1".into(),
        role: Role::Viewer,
        method: AuthMethod::Bearer,
    };
    assert!(svc.authorize(&p, Permission::UserAdmin).is_err());
}
