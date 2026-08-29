//! NCMD Node Control/Rebirth republishes NBIRTH + DBIRTH.

use plc_scan::ModeRequest;
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
    )
    .unwrap();
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

#[test]
fn catalog_replace_while_born_publishes_ddeath_then_dbirth() {
    let (_engine, src, _handle, _) = common::tiny_engine();
    let ids = TopicIds::new("g", "edge", "line").unwrap();
    let ddeath = ids.ddeath();
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
    )
    .unwrap();
    pubr.prepare_connect();
    pubr.on_connected().unwrap();
    let before = pubr.transport().publishes.len();

    pubr.set_catalog(
        TagCatalog::from_tags(vec![CatalogTag {
            name: "Q0".into(),
            value_type: MetricType::Bool,
            is_input: false,
            slot: 0,
            unit: String::new(),
        }])
        .unwrap(),
    )
    .unwrap();

    let after = &pubr.transport().publishes[before..];
    assert_eq!(after.len(), 2, "expected DDEATH then DBIRTH: {after:?}");
    assert_eq!(after[0].topic, ddeath);
    assert_eq!(after[1].topic, dbirth);
    let db = Payload::decode(&after[1].payload).unwrap();
    assert_eq!(db.seq, Some(0));
    assert_eq!(db.metrics[0].name.as_deref(), Some("Q0"));
}

#[test]
fn rebirth_dbirth_uses_cached_live_value() {
    let (mut engine, src, _handle, clock) = common::tiny_engine();
    let ids = TopicIds::new("g", "edge", "line").unwrap();
    let ncmd = ids.ncmd();
    let ddata = ids.ddata();
    let dbirth = ids.dbirth();
    let mut pubr = Publisher::new(
        ids,
        src,
        RecordingTransport::default(),
        MockWallClock::new(9, true),
        ConstMode(OperatingMode::Run),
    );
    pubr.set_catalog(TagCatalog::from_image_slots(2, 1).unwrap())
        .unwrap();
    pubr.prepare_connect();
    pubr.on_connected().unwrap();

    engine.request_mode(ModeRequest::Run);
    engine.step().unwrap();
    clock.advance_ms(50);
    pubr.drain().unwrap();

    let last_ddata = pubr
        .transport()
        .publishes
        .iter()
        .rev()
        .find(|f| f.topic == ddata)
        .expect("DDATA after drain");
    let ddata_payload = Payload::decode(&last_ddata.payload).unwrap();
    assert!(!ddata_payload.metrics.is_empty());
    let live = ddata_payload.metrics[0].clone();

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

    let rebirth_db = pubr
        .transport()
        .publishes
        .iter()
        .rev()
        .find(|f| f.topic == dbirth)
        .expect("DBIRTH after rebirth");
    let db = Payload::decode(&rebirth_db.payload).unwrap();
    let born = db
        .metrics
        .iter()
        .find(|m| m.alias == live.alias)
        .expect("rebirth DBIRTH includes the live alias");
    assert_eq!(born.value, live.value);
}
