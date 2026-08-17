//! Virtual-clock schedule and last/max µs.

mod common;

use plc_scan::{ModeRequest, StepOutcome};

#[test]
fn last_and_max_us_track_inject() {
    let (mut engine, _sim, clock) = common::arith_engine();
    engine.request_mode(ModeRequest::Run);
    engine.set_min_duration_us("main", Some(12_345)).unwrap();
    engine.step().unwrap();
    let t = &engine.status().tasks[0];
    assert_eq!(t.last_us, 12_345);
    assert_eq!(t.max_us, 12_345);
    assert_eq!(t.period_ms, 50);

    engine.set_min_duration_us("main", Some(1_000)).unwrap();
    clock.advance_ms(50);
    engine.step().unwrap();
    let t = &engine.status().tasks[0];
    assert_eq!(t.last_us, 1_000);
    assert_eq!(t.max_us, 12_345);
}

#[test]
fn idle_until_period_elapses() {
    let (mut engine, _sim, clock) = common::arith_engine();
    engine.request_mode(ModeRequest::Run);
    assert!(matches!(engine.step().unwrap(), StepOutcome::Ran { .. }));
    match engine.step().unwrap() {
        StepOutcome::Idle { next_due_ms } => assert_eq!(next_due_ms, 50),
        other @ StepOutcome::Ran { .. } => panic!("expected Idle, got {other:?}"),
    }
    clock.set_ms(50);
    assert!(matches!(engine.step().unwrap(), StepOutcome::Ran { .. }));
}

#[test]
fn multi_rate_periods_on_virtual_clock() {
    let (mut engine, _sim, clock) = common::multi_engine();
    engine.request_mode(ModeRequest::Run);
    engine.run_due().unwrap();

    clock.set_ms(20);
    let n = engine.run_due().unwrap();
    assert_eq!(n, 1);
    assert_eq!(engine.plan().tasks[engine.last_ran().unwrap()].name, "fast");

    clock.set_ms(50);
    let mut ran = Vec::new();
    while let StepOutcome::Ran { task, .. } = engine.step().unwrap() {
        ran.push(engine.plan().tasks[task].name.clone());
    }
    assert!(ran.contains(&"fast".to_string()) || ran.contains(&"main".to_string()));
}
