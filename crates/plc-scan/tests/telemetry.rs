//! TelemetrySource: CoS publish and backpressure does not block scan.

mod common;

use plc_io::{PlcValue, ProcessImage};
use plc_scan::{ModeRequest, ScanEngine, ScanIo, TelemetrySource};

fn tiny_tel_engine() -> (
    ScanEngine,
    common::SharedSim,
    TelemetrySource,
    plc_scan::VirtualClock,
) {
    let vm = common::vm_from_spasm(&common::arith_spasm());
    let sim = common::SharedSim::new(2, 1);
    let io = ScanIo::new(ProcessImage::with_sizes(2, 1, 0), Box::new(sim.clone()));
    let clock = plc_scan::VirtualClock::new();
    let mut plan = common::single_task_plan(50, "task.main");
    plan.telemetry_capacity = 4;
    plan.digital_cos_ms = 0;
    plan.analog_period_ms = 1;
    let engine = ScanEngine::new(plan, io, Some(vm), Box::new(clock.clone())).unwrap();
    let src = engine.telemetry_source();
    (engine, sim, src, clock)
}

#[test]
fn bool_change_of_state_enqueued() {
    let (mut engine, sim, src, clock) = tiny_tel_engine();
    sim.set_input(0, PlcValue::Bool(false));
    engine.request_mode(ModeRequest::Run);
    engine.step().unwrap();
    let mut samples = Vec::new();
    while let Some(s) = src.try_recv() {
        samples.push(s);
    }
    assert!(
        samples.iter().any(|s| s.is_input && s.tag_hint == 0),
        "first sample of I0 must publish"
    );

    // Unchanged BOOL + digital_cos_ms=0 still requires a change.
    clock.advance_ms(50);
    engine.step().unwrap();
    let mut second = Vec::new();
    while let Some(s) = src.try_recv() {
        second.push(s);
    }
    assert!(
        second.iter().all(|s| !(s.is_input && s.tag_hint == 0)),
        "unchanged BOOL must not republish: {second:?}"
    );

    sim.set_input(0, PlcValue::Bool(true));
    clock.advance_ms(50);
    engine.step().unwrap();
    let mut third = Vec::new();
    while let Some(s) = src.try_recv() {
        third.push(s);
    }
    assert!(third
        .iter()
        .any(|s| s.is_input && s.tag_hint == 0 && s.value == PlcValue::Bool(true)));
}

#[test]
fn backpressure_does_not_block_scan() {
    let (mut engine, sim, src, clock) = tiny_tel_engine();
    sim.set_input(0, PlcValue::Bool(true));
    sim.set_input(1, PlcValue::Bool(true));
    engine.request_mode(ModeRequest::Run);
    for i in 0..100 {
        // Flip a BOOL so every scan wants to publish.
        sim.set_input(0, PlcValue::Bool(i % 2 == 0));
        engine.step().expect("step must stay Ok under backpressure");
        clock.advance_ms(50);
    }
    assert!(
        src.drops() > 0,
        "capacity 4 over 100 steps must drop oldest"
    );
    // Drain remaining — must not panic / block.
    let mut n = 0;
    while src.try_recv().is_some() {
        n += 1;
        assert!(n <= 8, "ring must stay bounded");
    }
    assert!(n > 0);
}
