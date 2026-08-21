//! Scan-engine epoch hooks: skip rule, FirstScan, deferred install.

mod common;

use plc_scan::{ActivateRequest, ArmedProgram, ModeRequest, OutputRestartPolicy, StepOutcome};
use plc_types::{OperatingMode, ProgramPhase};
use plc_vm::{Vm, VmConfig};

fn armed(id: &str, spasm: &str, entries: &[&str], policy: OutputRestartPolicy) -> ArmedProgram {
    ArmedProgram {
        vm: Vm::from_spasm(spasm, &VmConfig::default()).expect("load"),
        program_id: id.into(),
        compatibility_hash: format!("{id}-hash"),
        task_entries: entries.iter().map(|s| (*s).to_string()).collect(),
        restart_policy: policy,
        retain_copies: Vec::new(),
    }
}

#[test]
fn skip_lower_priority_due_after_install() {
    let (mut engine, _sim, _clock) = common::multi_engine();
    engine
        .arm(armed(
            "v2",
            common::multi_spasm(),
            &["task.fast", "task.main", "task.slow"],
            OutputRestartPolicy::SafeReset,
        ))
        .unwrap();
    engine.request_mode(ModeRequest::Run);
    engine.request_activate().unwrap();

    let n = engine.run_due().unwrap();
    // Fast ran on new program; Main and Slow skipped their due tick.
    assert_eq!(n, 1);
    assert_eq!(engine.plan().tasks[engine.last_ran().unwrap()].name, "fast");
    assert_eq!(engine.current_program_id(), Some("v2"));
    assert_eq!(engine.epoch_hooks().phase(), ProgramPhase::Idle);
}

#[test]
fn first_scan_multi_rate_fast_clears_before_slow() {
    let (mut engine, _sim, clock) = common::multi_engine();
    engine
        .arm(armed(
            "v2",
            common::multi_spasm(),
            &["task.fast", "task.main", "task.slow"],
            OutputRestartPolicy::SafeReset,
        ))
        .unwrap();
    engine.request_mode(ModeRequest::Run);
    engine.request_activate().unwrap();
    engine.run_due().unwrap(); // Fast first post-activate (FirstScan true → cleared)

    let fast = 0;
    let slow = 2;
    assert!(!engine.epoch_hooks().first_scan(fast));
    assert!(engine.epoch_hooks().first_scan(slow));

    let mut fast_false = 0u32;
    for _ in 0..4 {
        clock.advance_ms(20);
        match engine.step().unwrap() {
            StepOutcome::Ran { task, .. } => {
                assert_ne!(engine.plan().tasks[task].name, "slow");
                if task == fast {
                    assert!(!engine.epoch_hooks().first_scan(fast));
                    assert!(engine.epoch_hooks().first_scan(slow));
                    fast_false += 1;
                }
            }
            StepOutcome::Idle { .. } => {}
        }
    }
    assert!(
        fast_false >= 2,
        "Fast must run ≥2 times with FirstScan=false before Slow: {fast_false}"
    );
    assert!(engine.epoch_hooks().first_scan(slow));
}

#[test]
fn activate_deferred_on_slow_install_keeps_old_program() {
    let (mut engine, _sim, clock) = common::arith_engine();
    engine
        .arm(armed(
            "v2",
            &common::arith_spasm(),
            &["task.main"],
            OutputRestartPolicy::SafeReset,
        ))
        .unwrap();
    engine.request_mode(ModeRequest::Run);
    // period is 50 ms → deadline 50_000 us; inject equal-or-over to defer.
    engine.set_install_min_duration_us(50_000);
    engine.request_activate().unwrap();
    engine.step().unwrap();
    assert!(engine.last_activate_deferred());
    assert_eq!(engine.epoch_hooks().phase(), ProgramPhase::Armed);
    assert_eq!(engine.current_program_id(), None); // arith_engine was constructed with a VM but no package id
    assert_eq!(engine.armed_program_id(), Some("v2"));
    assert_ne!(engine.mode(), OperatingMode::Fault);

    engine.set_install_min_duration_us(0);
    engine.request_activate().unwrap();
    clock.advance_ms(50);
    engine.step().unwrap();
    assert!(!engine.last_activate_deferred());
    assert_eq!(engine.current_program_id(), Some("v2"));
}

#[test]
fn same_id_and_hash_activate_is_noop() {
    let (mut engine, _sim, clock) = common::arith_engine();
    engine
        .arm(armed(
            "v1",
            &common::arith_spasm(),
            &["task.main"],
            OutputRestartPolicy::SafeReset,
        ))
        .unwrap();
    engine.request_mode(ModeRequest::Run);
    engine.request_activate().unwrap();
    engine.step().unwrap();
    assert_eq!(engine.current_program_id(), Some("v1"));

    engine
        .arm(armed(
            "v1",
            &common::arith_spasm(),
            &["task.main"],
            OutputRestartPolicy::SafeReset,
        ))
        .unwrap();
    assert_eq!(engine.request_activate().unwrap(), ActivateRequest::NoOp);
    assert_eq!(engine.epoch_hooks().phase(), ProgramPhase::Idle);
    assert!(engine.armed_program_id().is_none());
    clock.advance_ms(50);
    engine.step().unwrap();
    assert!(!engine.epoch_hooks().first_scan(0));
}

#[test]
fn validation_path_never_used_fault_from_arm() {
    // Arm is the success path; this just documents that a refused arm leaves mode.
    let (mut engine, _sim, _clock) = common::arith_engine();
    engine.request_mode(ModeRequest::Run);
    engine.step().unwrap();
    assert_eq!(engine.mode(), OperatingMode::Run);
    let err = engine.arm(armed(
        "bad",
        r#"
.header data_size=8 retain_size=0 input_slots=9 output_slots=1
.entry task.main
HALT
"#,
        &["task.main"],
        OutputRestartPolicy::SafeReset,
    ));
    assert!(err.is_err());
    assert_eq!(engine.mode(), OperatingMode::Run);
    assert_eq!(engine.epoch_hooks().phase(), ProgramPhase::Idle);
}
