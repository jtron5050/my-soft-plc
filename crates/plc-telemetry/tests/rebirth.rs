//! NCMD Node Control/Rebirth republishes NBIRTH + DBIRTH.

use plc_telemetry::{
    CatalogTag, ConstMode, MetricType, MockWallClock, Payload, Publisher, RecordingTransport,
    TagCatalog, TopicIds, METRIC_REBIRTH,
};
use plc_types::OperatingMode;

mod common;

#[test]
fn ncmd_rebirth_republishes_birth() {
    let (_engine, src, _handle, _) = common::tiny_engine();
    let ids = TopicIds::new("g", "edge", "line").unwrap();
    let ncmd = ids.ncmd();
    let nbirth = ids.nbirth();
    let dbirth = ids.dbirth();
    let mut pubr = Publisher::new(
        ids,
        src,
        RecordingTransport::default(),
        MockWallClock::new(9, true),
        ConstMode(OperatingMode::Run),
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
    let before = pubr.transport().publishes.len();

    let mut m = plc_telemetry::Metric::new(11);
    m.name = Some(METRIC_REBIRTH.into());
    m.value = Some(plc_telemetry::MetricValue::Bool(true));
    let cmd = Payload {
        timestamp: Some(9),
        metrics: vec![m],
        seq: None,
    }
    .encode();
    pubr.handle_incoming(&ncmd, &cmd).unwrap();
    let after = &pubr.transport().publishes[before..];
    assert!(after.iter().any(|f| f.topic == nbirth));
    assert!(after.iter().any(|f| f.topic == dbirth));
    let rebirth_nbirth = after.iter().find(|f| f.topic == nbirth).unwrap();
    let p = Payload::decode(&rebirth_nbirth.payload).unwrap();
    assert_eq!(p.seq, Some(0));
}
