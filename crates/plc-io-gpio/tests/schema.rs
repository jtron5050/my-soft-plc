//! Golden io-map checks and GPIO binding rejections.

use plc_io::{BadQualityPolicy, IoMap};
use plc_io_gpio::{validate_map, Bias, Drive, LineDirection};

fn validate(yaml_modules: &str) -> Result<plc_io_gpio::GpioPlan, plc_io::IoError> {
    let text = format!("version: 1\nmodules:\n{yaml_modules}");
    let map = IoMap::from_yaml_str(&text)?;
    let resolved = map.resolve()?;
    validate_map(&map, &resolved)
}

#[test]
fn golden_bench_matches_architecture_sample() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/configs/gpio-bench-io-map.yaml");
    let map = IoMap::load_from_path(&path).expect("golden map");
    let resolved = map.resolve().expect("resolve");
    let plan = validate_map(&map, &resolved).expect("validate");
    assert_eq!(plan.modules.len(), 2);

    let di = &plan.modules[0];
    assert_eq!(di.id, "local_di_1");
    assert_eq!(di.chip, "gpiochip0");
    assert_eq!(di.path.as_os_str(), "/dev/gpiochip0");
    assert_eq!(di.direction, LineDirection::Input);
    assert_eq!(di.offsets, vec![0, 1, 2, 3]);
    assert!(!di.active_low);
    assert_eq!(di.bias, Bias::AsIs);
    assert_eq!(di.drive, None);
    assert_eq!(di.points.len(), 1);
    assert_eq!(di.points[0].tag, "Conveyor1.PullCordOK");
    assert_eq!(di.points[0].bit, 0);
    assert_eq!(di.points[0].offset, 0);
    assert_eq!(di.points[0].slot, 0);
    assert!(!di.points[0].safe);

    let dout = &plan.modules[1];
    assert_eq!(dout.id, "local_do_1");
    assert_eq!(dout.chip, "gpiochip1");
    assert_eq!(dout.direction, LineDirection::Output);
    assert_eq!(dout.offsets, vec![0, 1]);
    assert_eq!(dout.drive, Some(Drive::OpenDrain));
    assert_eq!(dout.on_bad, BadQualityPolicy::ForceSafe);
    assert_eq!(dout.points.len(), 1);
    assert_eq!(dout.points[0].tag, "Conveyor1.RunFwd");
    assert_eq!(dout.points[0].bit, 0);
    assert_eq!(dout.points[0].offset, 0);
    assert_eq!(dout.points[0].slot, 0);
    assert!(!dout.points[0].safe);
}

#[test]
fn skips_sim_and_modbus_modules() {
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
        register: 40001
  - id: local_do_1
    driver: gpio
    config:
      chip: /dev/gpiochip2
      lines: [3]
      active_low: true
      bias: pull_down
      drive: push_pull
    bindings:
      - tag: Run
        image: Q
        safe_state: true
"#,
    )
    .unwrap();
    assert_eq!(plan.modules.len(), 1);
    let module = &plan.modules[0];
    assert_eq!(module.chip, "gpiochip2");
    assert!(module.active_low);
    assert_eq!(module.bias, Bias::PullDown);
    assert_eq!(module.drive, Some(Drive::PushPull));
    assert_eq!(module.points[0].bit, 0);
    assert_eq!(module.points[0].offset, 3);
    assert!(module.points[0].safe);
    assert_eq!(module.points[0].slot, 0);
}

#[test]
fn omitted_bit_takes_the_next_free_index() {
    let plan = validate(
        r#"
  - id: di
    driver: gpio
    config:
      chip: gpiochip0
      lines: [9, 8, 7]
    bindings:
      - tag: A
        image: I
        bit: 2
      - tag: B
        image: I
"#,
    )
    .unwrap();
    assert_eq!(plan.modules[0].points[0].bit, 2);
    assert_eq!(plan.modules[0].points[0].offset, 7);
    assert_eq!(plan.modules[0].points[1].bit, 0);
    assert_eq!(plan.modules[0].points[1].offset, 9);
}

#[test]
fn rejects_invalid_gpio_maps() {
    let cases = [
        (
            r#"
  - id: di
    driver: gpio
    config: { chip: gpiochip0, lines: [] }
    bindings:
      - tag: A
        image: I
"#,
            "non-empty",
        ),
        (
            r#"
  - id: di
    driver: gpio
    config:
      chip: gpiochip0
      lines: [0, 0]
    bindings:
      - tag: A
        image: I
"#,
            "duplicate line",
        ),
        (
            r#"
  - id: a
    driver: gpio
    config: { chip: gpiochip0, lines: [1] }
    bindings:
      - tag: A
        image: I
  - id: b
    driver: gpio
    config: { chip: /dev/gpiochip0, lines: [1] }
    bindings:
      - tag: B
        image: I
"#,
            "claimed",
        ),
        (
            r#"
  - id: both
    driver: gpio
    config: { chip: gpiochip0, lines: [0, 1] }
    bindings:
      - tag: In
        image: I
      - tag: Out
        image: Q
"#,
            "all inputs or all outputs",
        ),
        (
            r#"
  - id: mem
    driver: gpio
    config: { chip: gpiochip0, lines: [0] }
    bindings:
      - tag: M
        image: M
"#,
            "%M",
        ),
        (
            r#"
  - id: analog
    driver: gpio
    config: { chip: gpiochip0, lines: [0] }
    bindings:
      - tag: Level
        image: I
        type: REAL
"#,
            "BOOL",
        ),
        (
            r#"
  - id: reg
    driver: gpio
    config: { chip: gpiochip0, lines: [0] }
    bindings:
      - tag: Coil
        image: Q
        register: 1
"#,
            "registers",
        ),
        (
            r#"
  - id: scaled
    driver: gpio
    config: { chip: gpiochip0, lines: [0] }
    bindings:
      - tag: A
        image: I
        scale: 0.1
"#,
            "scale",
        ),
        (
            r#"
  - id: di
    driver: gpio
    config: { chip: gpiochip0, lines: [0] }
    bindings:
      - tag: A
        image: I
        bit: 3
"#,
            "outside lines",
        ),
        (
            r#"
  - id: di
    driver: gpio
    config: { chip: gpiochip0, lines: [0, 1] }
    bindings:
      - tag: A
        image: I
        bit: 0
      - tag: B
        image: I
        bit: 0
"#,
            "duplicate bit",
        ),
        (
            r#"
  - id: di
    driver: gpio
    config: { chip: ../gpiochip0, lines: [0] }
    bindings:
      - tag: A
        image: I
"#,
            "gpiochipN",
        ),
        (
            r#"
  - id: di
    driver: gpio
    config: { chip: gpiochip0, lines: [0], drive: open_drain }
    bindings:
      - tag: A
        image: I
"#,
            "drive is only valid",
        ),
        (
            r#"
  - id: di
    driver: gpio
    config: { chip: gpiochip0, lines: [0], poll_ms: 10 }
    bindings:
      - tag: A
        image: I
"#,
            "gpio config",
        ),
        (
            r#"
  - id: same
    driver: gpio
    config: { chip: gpiochip0, lines: [0] }
    bindings:
      - tag: A
        image: I
  - id: same
    driver: gpio
    config: { chip: gpiochip1, lines: [0] }
    bindings:
      - tag: B
        image: I
"#,
            "duplicate gpio module",
        ),
    ];
    for (yaml, needle) in cases {
        let err = validate(yaml).expect_err(needle);
        let text = err.to_string();
        assert!(text.contains(needle), "{needle} not in {text}");
    }
}

#[test]
fn rejects_more_than_64_lines() {
    let lines = (0..65)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let yaml = format!(
        r#"
  - id: di
    driver: gpio
    config:
      chip: gpiochip0
      lines: [{lines}]
    bindings:
      - tag: A
        image: I
        bit: 0
"#
    );
    let err = validate(&yaml).expect_err("65 lines");
    assert!(err.to_string().contains("exceed 64"), "{err}");
}
