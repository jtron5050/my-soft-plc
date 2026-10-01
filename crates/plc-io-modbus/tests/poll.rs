//! Poll worker against a loopback Modbus TCP peer.

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use plc_io::{FieldGate, InputUpdate, IoDriver, IoMap, OutputImage, PlcValue};
use plc_io_modbus::{validate_map, ModbusBridge};
use plc_types::{OperatingMode, Quality};

#[path = "common/mod.rs"]
mod common;

use common::{Mock, Server};

struct Harness {
    state: Arc<Mock>,
    /// Kept alive so the loopback peer outlives the bridge.
    #[allow(dead_code)]
    server: Server,
    gate: Arc<FieldGate>,
    bridge: ModbusBridge,
}

impl Harness {
    fn start(doc: &str, state: Arc<Mock>) -> Self {
        let server = Server::spawn(Arc::clone(&state));
        let yaml = doc.replace("{port}", &server.port().to_string());
        let map = IoMap::from_yaml_str(&yaml).expect("map");
        let resolved = map.resolve().expect("resolve");
        let plan = validate_map(&map, &resolved).expect("plan");
        let gate = Arc::new(FieldGate::new());
        let mut bridge = ModbusBridge::new(plan, Arc::clone(&gate));
        bridge.start().expect("start");
        Self {
            state,
            server,
            gate,
            bridge,
        }
    }

    fn input(&mut self) -> (PlcValue, Quality) {
        let mut update = InputUpdate::zeros(4);
        self.bridge.poll_inputs(&mut update).expect("poll");
        (update.values[0], update.quality[0])
    }
}

fn analog_yaml(policy: &str) -> String {
    format!(
        r#"
version: 1
modules:
  - id: rack
    driver: modbus_tcp
    config:
      endpoint: "127.0.0.1:{{port}}"
      unit: 1
      poll_ms: 30
      stale_ms: 400
      timeout_ms: 100
    on_bad_quality: {policy}
    bindings:
      - tag: Level
        image: I
        type: REAL
        register: 40001
        raw_type: INT
        scale: 0.1
      - tag: Gate
        image: Q
        type: BOOL
        register: 1
        safe_state: false
"#
    )
}

fn wait_until(timeout: Duration, mut pred: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if pred() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn scaled_holding_register_publishes_good() {
    let state = Mock::new();
    state.holdings.lock().unwrap()[0] = 675;
    let mut harness = Harness::start(&analog_yaml("force_safe"), state);
    harness.gate.set_mode(OperatingMode::Run);
    assert!(
        wait_until(Duration::from_secs(2), || {
            let (value, quality) = harness.input();
            quality == Quality::Good && value == PlcValue::Real(67.5)
        }),
        "timed out waiting for scaled input"
    );
}

#[test]
fn timeout_marks_bad_and_holds_last_value() {
    let state = Mock::new();
    state.holdings.lock().unwrap()[0] = 675;
    let mut harness = Harness::start(&analog_yaml("force_safe"), Arc::clone(&state));
    harness.gate.set_mode(OperatingMode::Run);
    assert!(wait_until(Duration::from_secs(2), || {
        let (value, quality) = harness.input();
        quality == Quality::Good && value == PlcValue::Real(67.5)
    }));
    state
        .hang_reads
        .store(true, std::sync::atomic::Ordering::Release);
    assert!(
        wait_until(Duration::from_secs(2), || harness.input().1 == Quality::Bad),
        "quality stayed good"
    );
    assert_eq!(harness.input().0, PlcValue::Real(67.5));
}

#[test]
fn force_safe_writes_coil_off() {
    let state = Mock::new();
    state.holdings.lock().unwrap()[0] = 1;
    let mut harness = Harness::start(&analog_yaml("force_safe"), Arc::clone(&state));
    harness.gate.set_mode(OperatingMode::Run);
    assert!(wait_until(Duration::from_secs(2), || {
        harness.input().1 == Quality::Good
    }));
    harness
        .bridge
        .apply_outputs(&OutputImage {
            values: vec![PlcValue::Bool(true)],
            force_safe: true,
        })
        .unwrap();
    assert!(
        wait_until(Duration::from_secs(2), || {
            harness
                .state
                .writes
                .lock()
                .unwrap()
                .iter()
                .any(|w| w.addr == 0 && w.coils == [false] && w.regs.is_empty())
        }),
        "safe coil was not written"
    );
}

#[test]
fn hold_last_does_not_write_while_bad() {
    let state = Mock::new();
    state.holdings.lock().unwrap()[0] = 1;
    let mut harness = Harness::start(&analog_yaml("hold_last"), Arc::clone(&state));
    harness.gate.set_mode(OperatingMode::Run);
    assert!(wait_until(Duration::from_secs(2), || {
        harness.input().1 == Quality::Good
    }));
    state
        .hang_reads
        .store(true, std::sync::atomic::Ordering::Release);
    assert!(wait_until(Duration::from_secs(2), || {
        harness.input().1 == Quality::Bad
    }));
    state.writes.lock().unwrap().clear();
    harness
        .bridge
        .apply_outputs(&OutputImage {
            values: vec![PlcValue::Bool(true)],
            force_safe: false,
        })
        .unwrap();
    thread::sleep(Duration::from_millis(350));
    assert!(
        state.writes.lock().unwrap().is_empty(),
        "hold_last wrote {:?}",
        state.writes.lock().unwrap().len()
    );
}

#[test]
fn sim_suppresses_field_writes() {
    let state = Mock::new();
    state.holdings.lock().unwrap()[0] = 1;
    let mut harness = Harness::start(&analog_yaml("force_safe"), Arc::clone(&state));
    assert!(wait_until(Duration::from_secs(2), || {
        harness.input().1 == Quality::Good
    }));
    harness.gate.set_mode(OperatingMode::Sim);
    state.writes.lock().unwrap().clear();
    harness
        .bridge
        .apply_outputs(&OutputImage {
            values: vec![PlcValue::Bool(true)],
            force_safe: false,
        })
        .unwrap();
    thread::sleep(Duration::from_millis(250));
    assert!(state.writes.lock().unwrap().is_empty());
    let mut update = InputUpdate::zeros(1);
    update.values[0] = PlcValue::Real(1.0);
    update.quality[0] = Quality::Good;
    harness.bridge.poll_inputs(&mut update).unwrap();
    assert_eq!(update.values[0], PlcValue::Real(1.0));
    harness.gate.set_mode(OperatingMode::Run);
    harness
        .bridge
        .apply_outputs(&OutputImage {
            values: vec![PlcValue::Bool(true)],
            force_safe: false,
        })
        .unwrap();
    assert!(wait_until(Duration::from_secs(2), || {
        state
            .writes
            .lock()
            .unwrap()
            .iter()
            .any(|w| w.coils == [true] && w.regs.is_empty())
    }));
}

#[test]
fn poll_inputs_does_not_block_on_a_stalled_socket() {
    let state = Mock::new();
    state
        .stall_conn
        .store(true, std::sync::atomic::Ordering::Release);
    let yaml = r#"
version: 1
modules:
  - id: rack
    driver: modbus_tcp
    config:
      endpoint: "127.0.0.1:{port}"
      unit: 1
      poll_ms: 40
      stale_ms: 2000
      timeout_ms: 800
    bindings:
      - tag: Level
        image: I
        type: INT
        register: 40001
"#;
    let mut harness = Harness::start(yaml, Arc::clone(&state));
    assert!(wait_until(Duration::from_secs(2), || {
        state.accepts.load(std::sync::atomic::Ordering::Acquire) >= 1
    }));
    let started = Instant::now();
    let mut update = InputUpdate::zeros(1);
    harness.bridge.poll_inputs(&mut update).unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(50),
        "poll_inputs blocked for {:?}",
        started.elapsed()
    );
    assert_eq!(update.quality[0], Quality::Bad);
}

#[test]
fn exception_and_transaction_id_mismatch() {
    let bad = Mock::new();
    bad.exception
        .store(true, std::sync::atomic::Ordering::Release);
    let mut harness = Harness::start(&analog_yaml("force_safe"), Arc::clone(&bad));
    assert!(wait_until(Duration::from_secs(2), || {
        harness.bridge.diagnostics().fail_count > 0
    }));
    assert_eq!(harness.input().1, Quality::Bad);
    drop(harness);

    let once = Mock::new();
    once.holdings.lock().unwrap()[0] = 10;
    once.wrong_once
        .store(true, std::sync::atomic::Ordering::Release);
    let mut harness = Harness::start(&analog_yaml("force_safe"), Arc::clone(&once));
    assert!(
        wait_until(Duration::from_secs(2), || harness.input().1
            == Quality::Good),
        "mismatched transaction id was not retried"
    );

    let always = Mock::new();
    always
        .always_wrong
        .store(true, std::sync::atomic::Ordering::Release);
    let mut harness = Harness::start(&analog_yaml("force_safe"), always);
    thread::sleep(Duration::from_millis(300));
    assert_eq!(harness.input().1, Quality::Bad);
    assert!(harness.bridge.diagnostics().fail_count > 0);
}

#[test]
fn grouped_coil_write_uses_fc15() {
    let state = Mock::new();
    let yaml = r#"
version: 1
modules:
  - id: rack
    driver: modbus_tcp
    config:
      endpoint: "127.0.0.1:{port}"
      unit: 1
      poll_ms: 30
      timeout_ms: 100
    bindings:
      - tag: A
        image: Q
        type: BOOL
        register: 1
        safe_state: false
      - tag: B
        image: Q
        type: BOOL
        register: 2
        safe_state: false
"#;
    let mut harness = Harness::start(yaml, Arc::clone(&state));
    harness.gate.set_mode(OperatingMode::Run);
    harness
        .bridge
        .apply_outputs(&OutputImage {
            values: vec![PlcValue::Bool(true), PlcValue::Bool(false)],
            force_safe: false,
        })
        .unwrap();
    assert!(wait_until(Duration::from_secs(2), || {
        state
            .writes
            .lock()
            .unwrap()
            .iter()
            .any(|w| w.fc == 15 && w.addr == 0 && w.coils == [true, false] && w.regs.is_empty())
    }));
}
