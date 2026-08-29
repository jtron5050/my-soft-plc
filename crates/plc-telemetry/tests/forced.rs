//! Forced=true property on DDATA.

use plc_io::PlcValue;
use plc_scan::TelemetrySample;
use plc_telemetry::{CatalogTag, MetricType, MetricValue, SessionState, TagCatalog};
use plc_types::Quality;

#[test]
fn forced_property_on_ddata() {
    let mut session = SessionState::new();
    session.set_catalog(
        TagCatalog::from_tags(vec![CatalogTag {
            name: "Q0".into(),
            value_type: MetricType::Bool,
            is_input: false,
            slot: 0,
            unit: String::new(),
        }])
        .unwrap(),
    );
    let sample = TelemetrySample {
        alias: 0,
        tag_hint: 0,
        value: PlcValue::Bool(true),
        quality: Quality::Good,
        forced: true,
        now_ms: 0,
        is_input: false,
    };
    let payload = session.ddata(1000, &[sample], true).unwrap();
    let forced = payload.metrics[0]
        .properties
        .iter()
        .find(|p| p.key == "Forced")
        .expect("Forced property");
    assert_eq!(forced.value, MetricValue::Bool(true));
}
