//! KD-19: unsynced wall clock forces Uncertain quality.

use plc_telemetry::{
    CatalogTag, ConstMode, MetricType, MetricValue, MockWallClock, Payload, Publisher,
    RecordingTransport, TagCatalog, TopicIds, QUALITY_UNCERTAIN,
};
use plc_types::OperatingMode;

mod common;

#[test]
fn unsynced_clock_marks_uncertain() {
    let (_engine, src, _handle, _clock) = common::tiny_engine();
    let ids = TopicIds::new("plantA", "n", "d").unwrap();
    let mut pubr = Publisher::new(
        ids,
        src,
        RecordingTransport::default(),
        MockWallClock::new(1_700_000_000_000, false),
        ConstMode(OperatingMode::Stop),
    );
    pubr.set_catalog(
        TagCatalog::from_tags(vec![CatalogTag {
            name: "I0".into(),
            value_type: MetricType::Bool,
            is_input: true,
            slot: 0,
            unit: String::new(),
        }])
        .unwrap(),
    );
    pubr.prepare_connect();
    pubr.on_connected().unwrap();
    let nbirth = &pubr.transport().publishes[0];
    let payload = Payload::decode(&nbirth.payload).unwrap();
    let mode = payload
        .metrics
        .iter()
        .find(|m| m.name.as_deref() == Some("SYSTEM/Mode"))
        .unwrap();
    let q = mode.properties.iter().find(|p| p.key == "Quality").unwrap();
    assert_eq!(q.value, MetricValue::Int(QUALITY_UNCERTAIN as u32));
}
