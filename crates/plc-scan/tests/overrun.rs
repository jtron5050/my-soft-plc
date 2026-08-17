//! Software watchdog: 50 ms injection on the 50 ms main task.

mod common;

use plc_io::PlcValue;
use plc_scan::ModeRequest;
use plc_types::{OperatingMode, Quality};

#[test]
fn two_consecutive_50ms_overruns_fault() {
    let (mut engine, sim, clock) = common::arith_engine();
    sim.set_input(0, PlcValue::Bool(true));
    sim.set_input(1, PlcValue::Bool(true));
    engine.request_mode(ModeRequest::Run);
    engine.set_min_duration_us("main", Some(50_000)).unwrap();

    engine.step().unwrap();
    assert_eq!(engine.mode(), OperatingMode::Run);
    assert_eq!(engine.status().tasks[0].overruns, 1);
    assert_eq!(engine.status().tasks[0].consecutive_overruns, 1);

    clock.advance_ms(50);
    engine.step().unwrap();
    assert_eq!(engine.mode(), OperatingMode::Fault);
    assert!(sim.last_force_safe());
    assert_eq!(sim.last_outputs()[0], PlcValue::Bool(false));
}

#[test]
fn in_time_scan_resets_consecutive() {
    let (mut engine, _sim, clock) = common::arith_engine();
    engine.request_mode(ModeRequest::Run);
    engine.set_min_duration_us("main", Some(50_000)).unwrap();
    engine.step().unwrap();
    assert_eq!(engine.mode(), OperatingMode::Run);

    engine.set_min_duration_us("main", None).unwrap();
    clock.advance_ms(50);
    engine.step().unwrap();
    assert_eq!(engine.mode(), OperatingMode::Run);
    assert_eq!(engine.status().tasks[0].consecutive_overruns, 0);
    assert_eq!(engine.status().tasks[0].overruns, 1);
}

#[test]
fn bad_quality_does_not_fault() {
    let (mut engine, sim, _clock) = common::arith_engine();
    sim.set_quality(0, Quality::Bad);
    engine.request_mode(ModeRequest::Run);
    engine.step().unwrap();
    assert_eq!(engine.mode(), OperatingMode::Run);
    assert!(engine.status().io_degraded);
}

#[test]
fn missing_entry_faults() {
    let vm = common::vm_from_spasm(common::halt_only_spasm());
    let sim = common::SharedSim::new(0, 1);
    let io = plc_scan::ScanIo::new(plc_io::ProcessImage::with_sizes(0, 1, 0), Box::new(sim));
    let clock = plc_scan::VirtualClock::new();
    let mut plan = common::single_task_plan(50, "task.does_not_exist");
    plan.overrun_limit = 2;
    let mut engine = plc_scan::ScanEngine::new(plan, io, Some(vm), Box::new(clock)).unwrap();
    engine.request_mode(ModeRequest::Run);
    engine.step().unwrap();
    assert_eq!(engine.mode(), OperatingMode::Fault);
}
