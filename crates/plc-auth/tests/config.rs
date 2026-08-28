//! `AuthService::from_config` plus config-layer validation errors.

mod common;

use plc_auth::{hash_secret, hex_encode, AuthError, AuthService, Credential};
use plc_config::{load_from_str, AuthConfig, ConfigFormat, LimitsConfig, PrincipalConfig};

use common::{token_hex, ENG_SECRET, LOOPBACK};

#[test]
fn from_config_loads_principal() {
    let auth = AuthConfig {
        required: true,
        principals: vec![PrincipalConfig {
            id: "eng".into(),
            role: "engineer".into(),
            token_sha256: Some(token_hex(ENG_SECRET)),
            cert_sha256: None,
        }],
        ..AuthConfig::default()
    };
    let svc = AuthService::from_config(&auth, &LimitsConfig::default()).unwrap();
    let p = svc
        .authenticate(&Credential::Bearer(ENG_SECRET.into()), LOOPBACK)
        .unwrap();
    assert_eq!(p.id, "eng");
}

#[test]
fn from_config_rejects_required_empty() {
    let auth = AuthConfig {
        required: true,
        ..AuthConfig::default()
    };
    let err = AuthService::from_config(&auth, &LimitsConfig::default()).unwrap_err();
    assert!(matches!(err, AuthError::Config(_)));
    assert!(err.to_string().contains("at least one principal"));
}

#[test]
fn from_config_rejects_bad_hex() {
    let auth = AuthConfig {
        principals: vec![PrincipalConfig {
            id: "eng".into(),
            role: "engineer".into(),
            token_sha256: Some("zz".into()),
            cert_sha256: None,
        }],
        ..AuthConfig::default()
    };
    let err = AuthService::from_config(&auth, &LimitsConfig::default()).unwrap_err();
    assert!(matches!(err, AuthError::Config(_)));
}

#[test]
fn from_config_accepts_uppercase_hex() {
    let hex = hex_encode(&hash_secret(ENG_SECRET.as_bytes())).to_ascii_uppercase();
    let auth = AuthConfig {
        required: true,
        principals: vec![PrincipalConfig {
            id: "eng".into(),
            role: "ENGINEER".into(),
            token_sha256: Some(hex),
            cert_sha256: None,
        }],
        ..AuthConfig::default()
    };
    let svc = AuthService::from_config(&auth, &LimitsConfig::default()).unwrap();
    svc.authenticate(&Credential::Bearer(ENG_SECRET.into()), LOOPBACK)
        .unwrap();
}

#[test]
fn yaml_prod_without_auth_rejected() {
    let yaml = r#"
version: 1
profile: prod
device:
  id: x
scan:
  tasks:
    - name: main
      period_ms: 50
      entry: task.main
program:
  require_signature: true
auth:
  required: false
"#;
    let err = load_from_str(yaml, ConfigFormat::Yaml).unwrap_err();
    assert!(err.to_string().contains("auth.required=true"));
}

#[test]
fn from_config_rejects_duplicate_token_hash() {
    let hex = token_hex(ENG_SECRET);
    let auth = AuthConfig {
        required: true,
        principals: vec![
            PrincipalConfig {
                id: "eng".into(),
                role: "engineer".into(),
                token_sha256: Some(hex.clone()),
                cert_sha256: None,
            },
            PrincipalConfig {
                id: "view".into(),
                role: "viewer".into(),
                token_sha256: Some(hex.to_ascii_uppercase()),
                cert_sha256: None,
            },
        ],
        ..AuthConfig::default()
    };
    let err = AuthService::from_config(&auth, &LimitsConfig::default()).unwrap_err();
    assert!(matches!(err, AuthError::Config(_)));
    assert!(err.to_string().contains("duplicate token_sha256"));
}

#[test]
fn from_config_rejects_duplicate_cert_hash() {
    let hex = hex_encode(&hash_secret(b"same-cert"));
    let auth = AuthConfig {
        required: true,
        principals: vec![
            PrincipalConfig {
                id: "a".into(),
                role: "engineer".into(),
                token_sha256: None,
                cert_sha256: Some(hex.clone()),
            },
            PrincipalConfig {
                id: "b".into(),
                role: "viewer".into(),
                token_sha256: None,
                cert_sha256: Some(hex),
            },
        ],
        ..AuthConfig::default()
    };
    let err = AuthService::from_config(&auth, &LimitsConfig::default()).unwrap_err();
    assert!(matches!(err, AuthError::Config(_)));
    assert!(err.to_string().contains("duplicate cert_sha256"));
}

#[test]
fn yaml_duplicate_principal() {
    let hex = hex_encode(&hash_secret(b"x"));
    let yaml = format!(
        r#"
version: 1
device:
  id: x
scan:
  tasks:
    - name: main
      period_ms: 50
      entry: task.main
auth:
  principals:
    - id: eng
      role: engineer
      token_sha256: "{hex}"
    - id: eng
      role: viewer
      token_sha256: "{hex}"
"#
    );
    let err = load_from_str(&yaml, ConfigFormat::Yaml).unwrap_err();
    assert!(err.to_string().contains("duplicate id"));
}

#[test]
fn yaml_duplicate_token_hash() {
    let hex = hex_encode(&hash_secret(b"x"));
    let yaml = format!(
        r#"
version: 1
device:
  id: x
scan:
  tasks:
    - name: main
      period_ms: 50
      entry: task.main
auth:
  principals:
    - id: eng
      role: engineer
      token_sha256: "{hex}"
    - id: view
      role: viewer
      token_sha256: "{}"
"#,
        hex.to_ascii_uppercase()
    );
    let err = load_from_str(&yaml, ConfigFormat::Yaml).unwrap_err();
    assert!(err.to_string().contains("duplicate token_sha256"));
}

#[test]
fn yaml_accepts_role_trim_and_case() {
    let hex = hex_encode(&hash_secret(b"x"));
    let yaml = format!(
        r#"
version: 1
device:
  id: x
scan:
  tasks:
    - name: main
      period_ms: 50
      entry: task.main
auth:
  principals:
    - id: eng
      role: " Engineer "
      token_sha256: "{hex}"
"#
    );
    let cfg = load_from_str(&yaml, ConfigFormat::Yaml).unwrap();
    assert_eq!(cfg.auth.principals[0].role, " Engineer ");
}
