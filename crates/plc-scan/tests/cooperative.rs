//! Cooperative multi-task: priority and run-to-completion.

mod common;

use plc_scan::{ModeRequest, StepOutcome};

#[test]
fn all_due_runs_highest_priority_first() {
    let (mut engine, _sim, _clock) = common::multi_engine();
    engine.request_mode(ModeRequest::Run);

    let a = engine.step().unwrap();
    let b = engine.step().unwrap();
    let c = engine.step().unwrap();
    let names = [a, b, c].map(|o| match o {
        StepOutcome::Ran { task, .. } => engine.plan().tasks[task].name.as_str(),
        StepOutcome::Idle { .. } => "idle",
    });
    assert_eq!(names, ["fast", "main", "slow"]);
}

#[test]
fn run_due_at_t0_runs_all_in_priority_order() {
    let (mut engine, _sim, _clock) = common::multi_engine();
    engine.request_mode(ModeRequest::Run);
    let n = engine.run_due().unwrap();
    assert_eq!(n, 3);
    assert_eq!(engine.plan().tasks[engine.last_ran().unwrap()].name, "slow");
}

#[test]
fn when_fast_and_main_both_due_fast_wins() {
    let (mut engine, _sim, clock) = common::multi_engine();
    engine.request_mode(ModeRequest::Run);
    engine.run_due().unwrap(); // t=0 → next fast=20, main=50, slow=500

    clock.set_ms(100); // fast due 20,40,60,80,100; main due 50,100
                       // run_due should service Fast before Main on every collision
    let mut order = Vec::new();
    while let StepOutcome::Ran { task, .. } = engine.step().unwrap() {
        order.push(engine.plan().tasks[task].name.clone());
        if order.len() > 16 {
            break;
        }
    }
    // First of the catch-up burst at t=100 must be fast (higher priority).
    assert_eq!(order.first().map(String::as_str), Some("fast"));
    assert!(order.contains(&"main".to_string()));
    // Never see main immediately before a still-due fast at the start.
    let first_main = order.iter().position(|n| n == "main").unwrap();
    assert!(
        order[..first_main].iter().all(|n| n == "fast"),
        "main must not run while a higher-priority fast is still due: {order:?}"
    );
}

#[test]
fn invocation_is_run_to_completion() {
    // One step returns only after the task HALTs; last_ran is that task.
    let (mut engine, _sim, _clock) = common::multi_engine();
    engine.request_mode(ModeRequest::Run);
    let StepOutcome::Ran { task, .. } = engine.step().unwrap() else {
        panic!("expected Ran");
    };
    assert_eq!(engine.plan().tasks[task].name, "fast");
    assert_eq!(engine.last_ran(), Some(task));
    assert!(engine.epoch_hooks().is_quiet());
}
