//! Drain maps scan samples onto DDATA aliases.

use plc_config::{load_from_str, ConfigFormat};
use plc_scan::ModeRequest;
use plc_telemetry::{
    ConstMode, MockWallClock, Payload, Publisher, RecordingTransport, TagCatalog, TelemetryService,
    TopicIds, SPARKPLUG_QOS,
};
use plc_types::OperatingMode;

mod common;

#[test]
fn ddata_uses_alias_and_qos1() {
    let (mut engine, src, _handle, clock) = common::tiny_engine();
    let ids = TopicIds::new("plantA", "softplc-01", "line").unwrap();
    let ddata = ids.ddata();
    let mut pubr = Publisher::new(
        ids,
        src,
        RecordingTransport::default(),
        MockWallClock::new(42, true),
        ConstMode(OperatingMode::Run),
    );
    pubr.set_catalog(TagCatalog::from_image_slots(2, 1).unwrap());
    pubr.prepare_connect();
    pubr.on_connected().unwrap();

    engine.request_mode(ModeRequest::Run);
    engine.step().unwrap();
    clock.advance_ms(50);
    pubr.drain().unwrap();

    let frames: Vec<_> = pubr
        .transport()
        .publishes
        .iter()
        .filter(|f| f.topic == ddata)
        .collect();
    assert!(!frames.is_empty(), "expected DDATA after first scan");
    assert!(frames.iter().all(|f| f.qos == SPARKPLUG_QOS));
    let p = Payload::decode(&frames[0].payload).unwrap();
    assert!(p.metrics.iter().all(|m| m.name.is_none()));
    assert!(p.metrics.iter().all(|m| m.alias.is_some()));
}

#[test]
fn disabled_service_is_noop() {
    let (_engine, src, handle, _) = common::tiny_engine();
    let cfg = load_from_str(
        r#"
version: 1
device:
  id: softplc-01
scan:
  tasks:
    - name: main
      period_ms: 50
      entry: task.main
telemetry:
  enabled: false
  group_id: plantA
  device_id: line
"#,
        ConfigFormat::Yaml,
    )
    .expect("config");
    let svc = TelemetryService::from_config(&cfg, src, handle).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    rt.block_on(svc.run()).unwrap();
}
