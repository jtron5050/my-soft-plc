//! NBIRTH/NDEATH share bdSeq; DATA uses aliases only.

use plc_io::PlcValue;
use plc_scan::TelemetrySample;
use plc_telemetry::{MetricValue, Payload, SessionState, TagCatalog, METRIC_BDSEQ};
use plc_types::{OperatingMode, Quality};

#[test]
fn will_bdseq_matches_nbirth() {
    let mut s = SessionState::new();
    s.prepare_connect();
    s.set_catalog(TagCatalog::from_image_slots(1, 0).unwrap());
    let death = s.ndeath(50);
    let birth = s.nbirth(50, OperatingMode::Stop, 0, Quality::Good);
    let d_bd = death
        .metrics
        .iter()
        .find(|m| m.name.as_deref() == Some(METRIC_BDSEQ))
        .unwrap();
    let b_bd = birth
        .metrics
        .iter()
        .find(|m| m.name.as_deref() == Some(METRIC_BDSEQ))
        .unwrap();
    assert_eq!(d_bd.value, Some(MetricValue::Long(0)));
    assert_eq!(d_bd.value, b_bd.value);
    assert_eq!(birth.seq, Some(0));
}

#[test]
fn ddata_omits_name() {
    let mut s = SessionState::new();
    s.set_catalog(TagCatalog::from_image_slots(1, 0).unwrap());
    let sample = TelemetrySample {
        alias: 0,
        tag_hint: 0,
        value: PlcValue::Bool(true),
        quality: Quality::Good,
        forced: false,
        now_ms: 0,
        is_input: true,
    };
    let p = s.ddata(1, &[sample], true).unwrap();
    assert!(p.metrics[0].name.is_none());
    assert_eq!(p.metrics[0].alias, Some(1));
    let _ = Payload::default();
}

#[test]
fn dbirth_uses_cached_live_value() {
    let mut s = SessionState::new();
    s.set_catalog(TagCatalog::from_image_slots(1, 0).unwrap());
    let before = s.dbirth(1, true).unwrap();
    assert_eq!(before.metrics[0].value, Some(MetricValue::Bool(false)));
    let sample = TelemetrySample {
        alias: 0,
        tag_hint: 0,
        value: PlcValue::Bool(true),
        quality: Quality::Good,
        forced: true,
        now_ms: 0,
        is_input: true,
    };
    assert!(s.ddata(2, &[sample], true).is_some());
    let after = s.dbirth(3, true).unwrap();
    assert_eq!(after.metrics[0].value, Some(MetricValue::Bool(true)));
    assert!(after.metrics[0]
        .properties
        .iter()
        .any(|p| p.key == "Forced" && p.value == MetricValue::Bool(true)));
}
