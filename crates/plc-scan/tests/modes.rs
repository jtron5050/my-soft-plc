//! STOP / RUN / FAULT / SIM transitions and output policy.

mod common;

use plc_config::StopOutputPolicy;
use plc_io::{PlcValue, ProcessImage};
use plc_scan::{ModeRequest, RecordingWatchdog, ScanEngine, ScanIo};
use plc_types::OperatingMode;

#[test]
fn default_mode_is_stop_and_applies_safe() {
    let (mut engine, sim, _clock) = common::arith_engine();
    engine.step().unwrap();
    assert_eq!(engine.mode(), OperatingMode::Stop);
    assert!(sim.last_force_safe());
    assert_eq!(sim.last_outputs()[0], PlcValue::Bool(false));
}

#[test]
fn run_executes_logic() {
    let (mut engine, sim, _clock) = common::arith_engine();
    sim.set_input(0, PlcValue::Bool(true));
    sim.set_input(1, PlcValue::Bool(true));
    engine.request_mode(ModeRequest::Run);
    engine.step().unwrap();
    assert_eq!(engine.mode(), OperatingMode::Run);
    assert_eq!(sim.last_outputs()[0], PlcValue::Bool(true));
}

#[test]
fn sim_from_stop_executes() {
    let (mut engine, sim, _clock) = common::arith_engine();
    sim.set_input(0, PlcValue::Bool(true));
    sim.set_input(1, PlcValue::Bool(true));
    engine.request_mode(ModeRequest::Sim);
    engine.step().unwrap();
    assert_eq!(engine.mode(), OperatingMode::Sim);
    assert_eq!(sim.last_outputs()[0], PlcValue::Bool(true));
}

#[test]
fn sim_refused_from_run() {
    let (mut engine, _sim, _clock) = common::arith_engine();
    engine.request_mode(ModeRequest::Run);
    engine.step().unwrap();
    assert_eq!(engine.mode(), OperatingMode::Run);
    engine.request_mode(ModeRequest::Sim);
    engine.step().unwrap();
    assert_eq!(engine.mode(), OperatingMode::Run);
    assert!(engine.status().mode_rejected >= 1);
}

#[test]
fn fault_reset_goes_to_stop_not_run() {
    let (mut engine, sim, clock) = common::arith_engine();
    engine.request_mode(ModeRequest::Run);
    engine.set_min_duration_us("main", Some(50_000)).unwrap();
    engine.step().unwrap();
    clock.advance_ms(50);
    engine.step().unwrap();
    assert_eq!(engine.mode(), OperatingMode::Fault);
    assert!(sim.last_force_safe());

    engine.request_mode(ModeRequest::FaultReset);
    clock.advance_ms(50);
    engine.step().unwrap();
    assert_eq!(engine.mode(), OperatingMode::Stop);

    engine.request_mode(ModeRequest::Run);
    engine.set_min_duration_us("main", None).unwrap();
    sim.set_input(0, PlcValue::Bool(true));
    sim.set_input(1, PlcValue::Bool(true));
    clock.advance_ms(50);
    engine.step().unwrap();
    assert_eq!(engine.mode(), OperatingMode::Run);
}

#[test]
fn forces_cleared_on_stop() {
    let (mut engine, _sim, clock) = common::arith_engine();
    engine.io_mut().forces.set(0, PlcValue::Bool(true));
    assert!(!engine.io_mut().forces.is_empty());
    engine.request_mode(ModeRequest::Run);
    engine.step().unwrap();
    engine.request_mode(ModeRequest::Stop);
    clock.advance_ms(50);
    engine.step().unwrap();
    assert!(engine.io_mut().forces.is_empty());
}

#[test]
fn stop_hold_keeps_last_program_q() {
    let vm = common::vm_from_spasm(&common::arith_spasm());
    let sim = common::SharedSim::new(2, 1);
    let io = ScanIo::new(ProcessImage::with_sizes(2, 1, 0), Box::new(sim.clone()));
    let clock = plc_scan::VirtualClock::new();
    let mut plan = common::single_task_plan(50, "task.main");
    plan.stop_output_policy = StopOutputPolicy::Hold;
    let mut engine = ScanEngine::new(plan, io, Some(vm), Box::new(clock.clone())).unwrap();

    sim.set_input(0, PlcValue::Bool(true));
    sim.set_input(1, PlcValue::Bool(true));
    engine.request_mode(ModeRequest::Run);
    engine.step().unwrap();
    assert_eq!(sim.last_outputs()[0], PlcValue::Bool(true));

    engine.request_mode(ModeRequest::Stop);
    clock.advance_ms(50);
    engine.step().unwrap();
    assert!(!sim.last_force_safe());
    assert_eq!(sim.last_outputs()[0], PlcValue::Bool(true));
}

#[test]
fn hw_watchdog_strokes_in_every_mode() {
    let vm = common::vm_from_spasm(&common::arith_spasm());
    let sim = common::SharedSim::new(2, 1);
    let io = ScanIo::new(ProcessImage::with_sizes(2, 1, 0), Box::new(sim.clone()));
    let clock = plc_scan::VirtualClock::new();
    let rec = RecordingWatchdog::new();
    let strokes = rec.strokes.clone();
    let mut engine = ScanEngine::new(
        common::single_task_plan(50, "task.main"),
        io,
        Some(vm),
        Box::new(clock.clone()),
    )
    .unwrap()
    .with_hw_watchdog(Box::new(rec));

    engine.step().unwrap(); // STOP
    engine.request_mode(ModeRequest::Run);
    clock.advance_ms(50);
    engine.step().unwrap();
    engine.request_mode(ModeRequest::Stop);
    clock.advance_ms(50);
    engine.step().unwrap();
    engine.request_mode(ModeRequest::Sim);
    clock.advance_ms(50);
    engine.step().unwrap();
    assert!(strokes.load(std::sync::atomic::Ordering::Relaxed) >= 4);
}

#[test]
fn stop_to_run_does_not_set_first_scan() {
    let (mut engine, _sim, _clock) = common::arith_engine();
    assert!(!engine.epoch_hooks().first_scan(0));
    engine.request_mode(ModeRequest::Run);
    engine.step().unwrap();
    assert!(!engine.epoch_hooks().first_scan(0));
}
