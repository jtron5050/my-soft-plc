//! arith-demo I→L→Q through SimDriver.

mod common;

use plc_io::PlcValue;
use plc_scan::{ModeRequest, StepOutcome};
use plc_types::Quality;

#[test]
fn arith_demo_and_gate() {
    let (mut engine, sim, _clock) = common::arith_engine();
    sim.set_input(0, PlcValue::Bool(true));
    sim.set_input(1, PlcValue::Bool(true));
    engine.request_mode(ModeRequest::Run);
    match engine.step().unwrap() {
        StepOutcome::Ran { task, .. } => assert_eq!(task, 0),
        other @ StepOutcome::Idle { .. } => panic!("expected Ran, got {other:?}"),
    }
    assert_eq!(sim.last_outputs()[0], PlcValue::Bool(true));
    assert!(!sim.last_force_safe());
}

#[test]
fn arith_demo_quality_gate() {
    let (mut engine, sim, _clock) = common::arith_engine();
    sim.set_input(0, PlcValue::Bool(true));
    sim.set_input(1, PlcValue::Bool(true));
    sim.set_quality(0, Quality::Bad);
    engine.request_mode(ModeRequest::Run);
    engine.step().unwrap();
    // Q0 := (I0 AND I1) AND I0.quality → false
    assert_eq!(sim.last_outputs()[0], PlcValue::Bool(false));
}

#[test]
fn arith_demo_false_input() {
    let (mut engine, sim, _clock) = common::arith_engine();
    sim.set_input(0, PlcValue::Bool(true));
    sim.set_input(1, PlcValue::Bool(false));
    engine.request_mode(ModeRequest::Run);
    engine.step().unwrap();
    assert_eq!(sim.last_outputs()[0], PlcValue::Bool(false));
}
