//! Golden protobuf bytes (Tahu field numbers).

use plc_telemetry::{is_rebirth_command, Metric, MetricValue, Payload, Property, SP_INT32};

#[test]
fn payload_timestamp_and_seq() {
    let p = Payload {
        timestamp: Some(1),
        metrics: Vec::new(),
        seq: Some(0),
    };
    assert_eq!(p.encode(), vec![0x08, 0x01, 0x18, 0x00]);
}

#[test]
fn quality_property_roundtrip() {
    let mut m = Metric::new(11);
    m.name = Some("t".into());
    m.alias = Some(1);
    m.value = Some(MetricValue::Bool(false));
    m.properties = vec![Property {
        key: "Quality".into(),
        datatype: SP_INT32,
        value: MetricValue::Int(192),
    }];
    let p = Payload {
        timestamp: Some(10),
        metrics: vec![m],
        seq: Some(1),
    };
    let back = Payload::decode(&p.encode()).unwrap();
    assert_eq!(back.metrics[0].properties[0].key, "Quality");
    assert_eq!(back.metrics[0].properties[0].value, MetricValue::Int(192));
}

#[test]
fn rebirth_payload_helper() {
    let mut m = Metric::new(11);
    m.name = Some("Node Control/Rebirth".into());
    m.value = Some(MetricValue::Bool(true));
    let bytes = Payload {
        timestamp: Some(1),
        metrics: vec![m],
        seq: None,
    }
    .encode();
    assert!(is_rebirth_command(&bytes));
}
