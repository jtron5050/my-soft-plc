//! MQTT backpressure must not block scan steps.

use plc_scan::ModeRequest;
use plc_telemetry::{
    ConstMode, MockWallClock, Publisher, RecordingTransport, TagCatalog, TopicIds,
};
use plc_types::OperatingMode;

mod common;

#[test]
fn scan_steps_while_mqtt_is_full() {
    let (mut engine, src, _handle, clock) = common::tiny_engine();
    let ids = TopicIds::new("g", "e", "d").unwrap();
    let transport = RecordingTransport {
        publish_full: true,
        ..RecordingTransport::default()
    };
    let mut pubr = Publisher::new(
        ids,
        src,
        transport,
        MockWallClock::new(1, true),
        ConstMode(OperatingMode::Run),
    );
    pubr.set_catalog(TagCatalog::from_image_slots(2, 1).unwrap());
    pubr.prepare_connect();
    pubr.on_connected().unwrap();
    assert!(pubr.mqtt_drops() > 0, "birth should count MQTT drops");

    engine.request_mode(ModeRequest::Run);
    for i in 0..40 {
        engine.step().expect("scan must not block");
        clock.advance_ms(50);
        pubr.drain().expect("drain must not fail on backpressure");
        let _ = i;
    }
    assert!(pubr.scan_drops() > 0 || pubr.mqtt_drops() > 0);
}
