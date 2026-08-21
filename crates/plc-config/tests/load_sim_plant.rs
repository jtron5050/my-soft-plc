//! Golden / sample config load tests.

use std::path::PathBuf;

use plc_config::{load_from_path, load_from_str, ConfigFormat, ProfileKind, StopOutputPolicy};

fn sample_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/configs/sim-plant.yaml")
}

#[test]
fn load_sim_plant_yaml() {
    let cfg = load_from_path(&sample_path()).expect("sim-plant.yaml must load");
    assert_eq!(cfg.version, 1);
    assert_eq!(cfg.profile, ProfileKind::Dev);
    assert_eq!(cfg.device.id, "softplc-sim-01");
    assert_eq!(cfg.scan.tasks.len(), 3);
    assert_eq!(cfg.scan.tasks[0].name, "fast");
    assert_eq!(cfg.scan.tasks[0].period_ms, 20);
    assert_eq!(cfg.scan.tasks[1].period_ms, 50);
    assert_eq!(cfg.telemetry.group_id, "plantA");
    assert_eq!(cfg.limits.max_package_bytes, 8 * 1024 * 1024);
    assert_eq!(cfg.stop_output_policy, StopOutputPolicy::Safe);
    assert!(cfg.io.drivers.iter().any(|d| d == "sim"));
    assert!(!cfg.program.require_signature);
    assert!(!cfg.auth.required);
    assert!(!cfg.auth.dual_control);
    assert_eq!(cfg.auth.lockout_secs, 60);
    assert!(cfg.auth.principals.is_empty());
}

#[test]
fn reject_empty_tasks() {
    let yaml = r#"
version: 1
device:
  id: x
scan:
  tasks: []
"#;
    let err = load_from_str(yaml, ConfigFormat::Yaml).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("scan.tasks must not be empty"),
        "unexpected: {msg}"
    );
}

#[test]
fn reject_duplicate_task_names() {
    let yaml = r#"
version: 1
device:
  id: x
scan:
  tasks:
    - name: main
      period_ms: 50
      entry: task.main
    - name: main
      period_ms: 100
      entry: task.slow
"#;
    let err = load_from_str(yaml, ConfigFormat::Yaml).unwrap_err();
    assert!(err.to_string().contains("duplicate task.name"));
}

#[test]
fn reject_zero_period() {
    let yaml = r#"
version: 1
device:
  id: x
scan:
  tasks:
    - name: main
      period_ms: 0
      entry: task.main
"#;
    let err = load_from_str(yaml, ConfigFormat::Yaml).unwrap_err();
    assert!(err.to_string().contains("period_ms must be > 0"));
}

#[test]
fn reject_unknown_driver() {
    let yaml = r#"
version: 1
device:
  id: x
scan:
  tasks:
    - name: main
      period_ms: 50
      entry: task.main
io:
  drivers: [ethercat]
"#;
    let err = load_from_str(yaml, ConfigFormat::Yaml).unwrap_err();
    assert!(err.to_string().contains("unknown driver"));
}

#[test]
fn reject_prod_without_signature() {
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
  require_signature: false
"#;
    let err = load_from_str(yaml, ConfigFormat::Yaml).unwrap_err();
    assert!(err.to_string().contains("require_signature=true"));
}

#[test]
fn reject_bad_version() {
    let yaml = r#"
version: 99
device:
  id: x
scan:
  tasks:
    - name: main
      period_ms: 50
      entry: task.main
"#;
    let err = load_from_str(yaml, ConfigFormat::Yaml).unwrap_err();
    assert!(err
        .to_string()
        .contains("unsupported config schema version"));
}

#[test]
fn reject_prod_without_auth() {
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
fn reject_required_without_principals() {
    let yaml = r#"
version: 1
device:
  id: x
scan:
  tasks:
    - name: main
      period_ms: 50
      entry: task.main
auth:
  required: true
"#;
    let err = load_from_str(yaml, ConfigFormat::Yaml).unwrap_err();
    assert!(err.to_string().contains("at least one principal"));
}

#[test]
fn reject_unknown_role() {
    let yaml = r#"
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
      role: superuser
      token_sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
"#;
    let err = load_from_str(yaml, ConfigFormat::Yaml).unwrap_err();
    assert!(err.to_string().contains("unknown role"));
}

#[test]
fn reject_principal_without_identity() {
    let yaml = r#"
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
"#;
    let err = load_from_str(yaml, ConfigFormat::Yaml).unwrap_err();
    assert!(err.to_string().contains("token_sha256 or cert_sha256"));
}

#[test]
fn reject_duplicate_principal_ids() {
    let hex = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
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
fn reject_bad_token_hash() {
    let yaml = r#"
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
      token_sha256: "not-a-hash"
"#;
    let err = load_from_str(yaml, ConfigFormat::Yaml).unwrap_err();
    assert!(err.to_string().contains("64 hex characters"));
}

#[test]
fn accept_principal_with_token() {
    let yaml = r#"
version: 1
device:
  id: x
scan:
  tasks:
    - name: main
      period_ms: 50
      entry: task.main
auth:
  required: true
  dual_control: true
  lockout_secs: 60
  principals:
    - id: eng
      role: engineer
      token_sha256: "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855"
"#;
    let cfg = load_from_str(yaml, ConfigFormat::Yaml).unwrap();
    assert!(cfg.auth.required);
    assert!(cfg.auth.dual_control);
    assert_eq!(cfg.auth.principals.len(), 1);
    assert_eq!(cfg.auth.principals[0].id, "eng");
}

#[test]
fn json_round_trip_minimal() {
    let yaml = std::fs::read_to_string(sample_path()).unwrap();
    let cfg = load_from_str(&yaml, ConfigFormat::Yaml).unwrap();
    let json = serde_json::to_string_pretty(&cfg).unwrap();
    let cfg2 = load_from_str(&json, ConfigFormat::Json).unwrap();
    assert_eq!(cfg.device.id, cfg2.device.id);
    assert_eq!(cfg.scan.tasks.len(), cfg2.scan.tasks.len());
}
