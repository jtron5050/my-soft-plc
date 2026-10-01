//! Golden io-map checks and Modbus binding rejections.

use std::time::Duration;

use plc_io::{BindingDirection, IoMap, RegisterType, ValueType};
use plc_io_modbus::validate_map;

fn validate(yaml_modules: &str) -> Result<plc_io_modbus::ModbusPlan, plc_io::IoError> {
    let text = format!("version: 1\nmodules:\n{yaml_modules}");
    let map = IoMap::from_yaml_str(&text)?;
    let resolved = map.resolve()?;
    validate_map(&map, &resolved)
}

#[test]
fn golden_rack_matches_architecture_sample() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/configs/modbus-rack-io-map.yaml");
    let map = IoMap::load_from_path(&path).expect("golden map");
    let resolved = map.resolve().expect("resolve");
    let plan = validate_map(&map, &resolved).expect("validate");
    assert_eq!(plan.modules.len(), 1);
    let module = &plan.modules[0];
    assert_eq!(module.id, "remote_rack_a");
    assert_eq!(module.host, "192.168.10.20");
    assert_eq!(module.port, 502);
    assert_eq!(module.unit, 1);
    assert_eq!(module.poll_ms, 50);
    assert_eq!(module.stale, Duration::from_millis(150));
    assert_eq!(module.timeout, Duration::from_millis(50));
    assert_eq!(module.on_bad, plc_io::BadQualityPolicy::ForceSafe);
    assert_eq!(module.points.len(), 2);
    let level = &module.points[0];
    assert_eq!(level.tag, "Silo1.Level_eu");
    assert_eq!(level.direction, BindingDirection::Input);
    assert_eq!(level.table, RegisterType::Holding);
    assert_eq!(level.pdu, 0);
    assert_eq!(level.value_type, ValueType::Real);
    assert!((level.scale - 0.1).abs() < 1e-12);
    assert_eq!(level.clamp, Some([0.0, 100.0]));
    assert_eq!(level.slot, 0);
    let gate = &module.points[1];
    assert_eq!(gate.table, RegisterType::Coil);
    assert_eq!(gate.pdu, 0);
    assert_eq!(gate.direction, BindingDirection::Output);
    assert_eq!(gate.safe, plc_io::PlcValue::Bool(false));
}

#[test]
fn skips_sim_modules() {
    let plan = validate(
        r#"
  - id: sim_line
    driver: sim
    bindings:
      - tag: Local
        image: I
  - id: rack
    driver: modbus_tcp
    config:
      endpoint: "127.0.0.1:502"
    bindings:
      - tag: Remote
        image: I
        type: INT
        register: 40010
"#,
    )
    .unwrap();
    assert_eq!(plan.modules.len(), 1);
    assert_eq!(plan.modules[0].points[0].pdu, 9);
    assert_eq!(plan.modules[0].poll_ms, 100);
    assert_eq!(plan.modules[0].stale, Duration::from_millis(300));
}

#[test]
fn rejects_invalid_modbus_maps() {
    let cases = [
        (
            r#"
  - id: rack
    driver: modbus_tcp
    config: { endpoint: "127.0.0.1:502", unit: 1 }
    bindings:
      - tag: A
        image: I
        register: 40001
        register_type: coil
"#,
            "not coil",
        ),
        (
            r#"
  - id: rack
    driver: modbus_tcp
    config: { endpoint: "127.0.0.1:502" }
    bindings:
      - tag: A
        image: I
        type: INT
"#,
            "register is required",
        ),
        (
            r#"
  - id: rack
    driver: modbus_tcp
    config: { endpoint: "127.0.0.1:502" }
    bindings:
      - tag: Wide
        image: I
        type: REAL
        register: 40001
        raw_type: REAL
      - tag: Next
        image: I
        type: INT
        register: 40002
        raw_type: INT
"#,
            "overlap",
        ),
        (
            r#"
  - id: rack
    driver: modbus_tcp
    config: { endpoint: "127.0.0.1:502", poll_ms: 0 }
    bindings:
      - tag: A
        image: I
        register: 1
"#,
            "poll_ms must be > 0",
        ),
        (
            r#"
  - id: rack
    driver: modbus_tcp
    config: { endpoint: "127.0.0.1:502", unit: 0 }
    bindings:
      - tag: A
        image: I
        register: 1
"#,
            "unit must be 1..=255",
        ),
        (
            r#"
  - id: rack
    driver: modbus_tcp
    config: { endpoint: "127.0.0.1" }
    bindings:
      - tag: A
        image: I
        register: 1
"#,
            "host:port",
        ),
        (
            r#"
  - id: rack
    driver: modbus_tcp
    config: { endpoint: "127.0.0.1:502" }
    bindings:
      - tag: Mem
        image: M
        register: 40001
"#,
            "%M",
        ),
        (
            r#"
  - id: rack
    driver: modbus_tcp
    config: { endpoint: "127.0.0.1:502" }
    bindings:
      - tag: T
        image: I
        type: TIME
        register: 40001
"#,
            "TIME",
        ),
        (
            r#"
  - id: rack
    driver: modbus_tcp
    config: { endpoint: "127.0.0.1:502" }
    bindings:
      - tag: Out
        image: Q
        type: REAL
        register: 40001
        raw_type: INT
        scale: 0
"#,
            "scale must be non-zero",
        ),
        (
            r#"
  - id: rack
    driver: modbus_tcp
    config: { endpoint: "127.0.0.1:502" }
    bindings:
      - tag: A
        image: I
        register: 0
"#,
            "5-digit",
        ),
        (
            r#"
  - id: rack
    driver: modbus_tcp
    config: { endpoint: "127.0.0.1:502", nope: 1 }
    bindings:
      - tag: A
        image: I
        register: 1
"#,
            "unknown field",
        ),
        (
            r#"
  - id: rack
    driver: modbus_tcp
    config: { endpoint: "127.0.0.1:502" }
    bindings:
      - tag: Packed
        image: I
        type: BOOL
        register: 40001
"#,
            "requires bit",
        ),
        (
            r#"
  - id: rack
    driver: modbus_tcp
    config: { endpoint: "127.0.0.1:502" }
    bindings:
      - tag: CoilBit
        image: I
        type: BOOL
        register: 1
        bit: 0
"#,
            "bit is not used",
        ),
        (
            r#"
  - id: rack
    driver: modbus_tcp
    config: { endpoint: "127.0.0.1:502" }
    bindings:
      - tag: InReg
        image: Q
        type: INT
        register: 30001
"#,
            "read-only",
        ),
        (
            r#"
  - id: rack
    driver: modbus_tcp
    config: { endpoint: "127.0.0.1:502", poll_ms: 50, stale_ms: 10 }
    bindings:
      - tag: A
        image: I
        register: 1
"#,
            "stale_ms must be >= poll_ms",
        ),
    ];
    for (yaml, needle) in cases {
        let err = validate(yaml).expect_err(needle);
        assert!(
            err.to_string().contains(needle),
            "expected '{needle}' in {err}"
        );
    }
}
