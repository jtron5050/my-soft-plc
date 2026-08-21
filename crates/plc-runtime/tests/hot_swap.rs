//! PR-10 policy tests: timer/PID/retain/restart_policy/deferred/FirstScan/STOP→RUN.

mod common;

use plc_fb_primitives::Pid;
use plc_io::ProcessImage;
use plc_ir::IrType;
use plc_package::RestartPolicy;
use plc_runtime::RuntimeConfig;
use plc_scan::{ModeRequest, ScanIo, StepOutcome};
use plc_types::{OperatingMode, ProgramPhase};

use common::{pack, runtime_multi, runtime_single, PackOpts, SharedSim};

const TON: &str = r#"
.header data_size=16 retain_size=0 input_slots=0 output_slots=0
.entry task.main
PUSHI_BOOL 1
PUSH_TIME  1000
CALL_FB    prim=TON instance=0
ST_DATA    0
ST_DATA    4
HALT
"#;

const PID: &str = r#"
.header data_size=16 retain_size=0 input_slots=0 output_slots=0
.entry task.main
PUSHI_REAL 0.0
PUSHI_REAL 1.0
PUSHI_BOOL 1
CALL_FB    prim=PID instance=0
ST_DATA    0
HALT
"#;

const RETAIN_WRITE: &str = r#"
.header data_size=8 retain_size=8 input_slots=0 output_slots=0
.entry task.main
PUSHI_DINT 42
ST_RETAIN  0
HALT
"#;

const RETAIN_READ: &str = r#"
.header data_size=8 retain_size=8 input_slots=0 output_slots=0
.entry task.main
LD_RETAIN  0
ST_DATA    0
HALT
"#;

const Q_TRUE: &str = r#"
.header data_size=8 retain_size=0 input_slots=0 output_slots=1
.entry task.main
PUSHI_BOOL 1
ST_Q       0
HALT
"#;

const Q_FALSE: &str = r#"
.header data_size=8 retain_size=0 input_slots=0 output_slots=1
.entry task.main
PUSHI_BOOL 0
ST_Q       0
HALT
"#;

const MULTI_NOP: &str = r#"
.header data_size=8 retain_size=0 input_slots=0 output_slots=2
.entry task.fast
HALT
.entry task.slow
HALT
"#;

fn ton_pkg(id: &str) -> Vec<u8> {
    pack(&PackOpts {
        id,
        spasm: TON,
        tasks: &[("main", "task.main")],
        retain: &[],
        restart: RestartPolicy::SafeReset,
        q_tags: &[],
    })
}

fn load_and_run(rt: &mut plc_runtime::Runtime, pkg: &[u8]) {
    rt.upload(pkg).unwrap();
    rt.activate().unwrap();
    rt.engine_mut().request_mode(ModeRequest::Run);
    rt.step().unwrap();
}

#[test]
fn timer_resets_on_activate() {
    let (mut rt, _sim, clock) = runtime_single(0, 0, 50);
    load_and_run(&mut rt, &ton_pkg("ton-a"));
    clock.advance_ms(50);
    rt.step().unwrap();
    clock.advance_ms(50);
    rt.step().unwrap();
    let et_before = rt.engine().vm().unwrap().primitives().ton[0].et;
    assert!(et_before > 0, "timer should have elapsed, et={et_before}");

    rt.upload(&ton_pkg("ton-b")).unwrap();
    rt.activate().unwrap();
    clock.advance_ms(50);
    rt.step().unwrap();
    let et_after = rt.engine().vm().unwrap().primitives().ton[0].et;
    assert_eq!(et_after, 0, "TON must cold-init on activate");
}

#[test]
fn pid_integrator_cold_on_activate() {
    let (mut rt, _sim, clock) = runtime_single(0, 0, 50);
    load_and_run(
        &mut rt,
        &pack(&PackOpts {
            id: "pid-a",
            spasm: PID,
            tasks: &[("main", "task.main")],
            retain: &[],
            restart: RestartPolicy::SafeReset,
            q_tags: &[],
        }),
    );
    rt.engine_mut().vm_mut().unwrap().primitives_mut().pid[0] =
        Pid::new(0.0, 1.0, 0.0, -100.0, 100.0);
    clock.advance_ms(50);
    rt.step().unwrap(); // first sample after reconfig is P-only
    clock.advance_ms(1000);
    rt.step().unwrap();
    let warm = &rt.engine().vm().unwrap().primitives().pid[0];
    assert!(
        warm.integral() > 0.5,
        "expected warm integrator, got {}",
        warm.integral()
    );
    assert!((warm.kp).abs() < 1e-6 && (warm.ki - 1.0).abs() < 1e-6);

    rt.upload(&pack(&PackOpts {
        id: "pid-b",
        spasm: PID,
        tasks: &[("main", "task.main")],
        retain: &[],
        restart: RestartPolicy::SafeReset,
        q_tags: &[],
    }))
    .unwrap();
    rt.activate().unwrap();
    clock.advance_ms(50);
    rt.step().unwrap();
    let pid = &rt.engine().vm().unwrap().primitives().pid[0];
    assert!(
        pid.integral().abs() < 1e-6,
        "PID integrator must be cold after activate, got {}",
        pid.integral()
    );
    assert!(
        (pid.kp - 1.0).abs() < 1e-6 && pid.ki.abs() < 1e-6,
        "new program must not keep the previous PID instance (kp={}, ki={})",
        pid.kp,
        pid.ki
    );
}

#[test]
fn retain_keep_same_path_and_type() {
    let (mut rt, _sim, clock) = runtime_single(0, 0, 50);
    let retain = &[("Hours", IrType::Dint, 0)];
    load_and_run(
        &mut rt,
        &pack(&PackOpts {
            id: "ret-a",
            spasm: RETAIN_WRITE,
            tasks: &[("main", "task.main")],
            retain,
            restart: RestartPolicy::SafeReset,
            q_tags: &[],
        }),
    );
    assert_eq!(
        rt.engine().vm().unwrap().retain().as_bytes()[0..4],
        42i32.to_le_bytes()
    );

    let report = rt
        .upload(&pack(&PackOpts {
            id: "ret-b",
            spasm: RETAIN_READ,
            tasks: &[("main", "task.main")],
            retain,
            restart: RestartPolicy::SafeReset,
            q_tags: &[],
        }))
        .unwrap();
    assert_eq!(report.retain.kept, 1);
    rt.activate().unwrap();
    clock.advance_ms(50);
    rt.step().unwrap();
    let data = rt.engine().vm().unwrap().data().as_bytes();
    assert_eq!(&data[0..4], &42i32.to_le_bytes());
}

#[test]
fn retain_reject_incompatible_type() {
    let (mut rt, _sim, _clock) = runtime_single(0, 0, 50);
    load_and_run(
        &mut rt,
        &pack(&PackOpts {
            id: "ret-a",
            spasm: RETAIN_WRITE,
            tasks: &[("main", "task.main")],
            retain: &[("Hours", IrType::Dint, 0)],
            restart: RestartPolicy::SafeReset,
            q_tags: &[],
        }),
    );
    let mode = rt.mode();
    let err = rt
        .upload(&pack(&PackOpts {
            id: "ret-bad",
            spasm: RETAIN_READ,
            tasks: &[("main", "task.main")],
            retain: &[("Hours", IrType::Int, 0)],
            restart: RestartPolicy::SafeReset,
            q_tags: &[],
        }))
        .unwrap_err();
    assert!(err.to_string().contains("incompatible"), "{err}");
    assert_eq!(rt.mode(), mode);
    assert_ne!(rt.mode(), OperatingMode::Fault);
    assert_eq!(rt.phase(), ProgramPhase::Idle);
    assert_eq!(rt.engine().current_program_id(), Some("ret-a"));
}

#[test]
fn restart_policy_bumpless_holds_q_safe_reset_does_not() {
    let q_tags = &[("Q0", 0)];
    let a = pack(&PackOpts {
        id: "q-a",
        spasm: Q_TRUE,
        tasks: &[("main", "task.main")],
        retain: &[],
        restart: RestartPolicy::SafeReset,
        q_tags,
    });
    let b_bumpless = pack(&PackOpts {
        id: "q-b",
        spasm: Q_FALSE,
        tasks: &[("main", "task.main")],
        retain: &[],
        restart: RestartPolicy::Bumpless,
        q_tags,
    });
    let b_safe = pack(&PackOpts {
        id: "q-c",
        spasm: Q_FALSE,
        tasks: &[("main", "task.main")],
        retain: &[],
        restart: RestartPolicy::SafeReset,
        q_tags,
    });

    let (mut rt, sim, clock) = runtime_single(0, 1, 20);
    load_and_run(&mut rt, &a);
    assert_eq!(sim.last_outputs()[0], plc_io::PlcValue::Bool(true));

    let report = rt.upload(&b_bumpless).unwrap();
    assert!(report.bumpless_honored);
    rt.activate().unwrap();
    clock.advance_ms(20);
    rt.step().unwrap();
    assert_eq!(
        sim.last_outputs()[0],
        plc_io::PlcValue::Bool(true),
        "bumpless holds last %Q through first post-activate invocation"
    );
    clock.advance_ms(20);
    rt.step().unwrap();
    assert_eq!(
        sim.last_outputs()[0],
        plc_io::PlcValue::Bool(false),
        "program drives %Q after first invocation"
    );

    // Fresh runtime for safe_reset contrast.
    let (mut rt, sim, clock) = runtime_single(0, 1, 20);
    load_and_run(&mut rt, &a);
    rt.upload(&b_safe).unwrap();
    rt.activate().unwrap();
    clock.advance_ms(20);
    rt.step().unwrap();
    assert_eq!(
        sim.last_outputs()[0],
        plc_io::PlcValue::Bool(false),
        "safe_reset does not hold last %Q"
    );
}

#[test]
fn activate_deferred_on_injected_slow_install() {
    let (mut rt, _sim, clock) = runtime_single(0, 0, 20);
    load_and_run(&mut rt, &ton_pkg("old"));
    rt.upload(&ton_pkg("new")).unwrap();
    rt.activate().unwrap();
    rt.engine_mut().set_install_min_duration_us(25_000); // > 20 ms Fast/main period
    clock.advance_ms(50);
    rt.step().unwrap();
    assert!(rt.engine().last_activate_deferred());
    assert_eq!(rt.phase(), ProgramPhase::Armed);
    assert_eq!(rt.engine().current_program_id(), Some("old"));
    assert_eq!(rt.engine().armed_program_id(), Some("new"));
    assert_ne!(rt.mode(), OperatingMode::Fault);

    rt.engine_mut().set_install_min_duration_us(0);
    rt.activate().unwrap();
    clock.advance_ms(50);
    rt.step().unwrap();
    assert!(!rt.engine().last_activate_deferred());
    assert_eq!(rt.engine().current_program_id(), Some("new"));
    assert_eq!(rt.phase(), ProgramPhase::Idle);
}

#[test]
fn first_scan_multi_rate() {
    let (mut rt, _sim, clock) = runtime_multi();
    let pkg = pack(&PackOpts {
        id: "mr",
        spasm: MULTI_NOP,
        tasks: &[("fast", "task.fast"), ("slow", "task.slow")],
        retain: &[],
        restart: RestartPolicy::SafeReset,
        q_tags: &[],
    });
    rt.upload(&pkg).unwrap();
    rt.activate().unwrap();
    rt.engine_mut().request_mode(ModeRequest::Run);
    rt.run_due().unwrap();

    let hooks = rt.engine().epoch_hooks();
    assert!(
        !hooks.first_scan(0),
        "Fast FirstScan cleared after first run"
    );
    assert!(hooks.first_scan(1), "Slow has not run yet");

    let mut fast_false = 0u32;
    for _ in 0..4 {
        clock.advance_ms(20);
        match rt.step().unwrap() {
            StepOutcome::Ran { task, .. } => {
                assert_ne!(rt.engine().plan().tasks[task].name, "slow");
                if task == 0 {
                    assert!(!rt.engine().epoch_hooks().first_scan(0));
                    assert!(rt.engine().epoch_hooks().first_scan(1));
                    fast_false += 1;
                }
            }
            StepOutcome::Idle { .. } => {}
        }
    }
    assert!(
        fast_false >= 2,
        "Fast runs ≥2 times with FirstScan=false before Slow: {fast_false}"
    );
}

#[test]
fn stop_to_run_preserves_timer() {
    let (mut rt, _sim, clock) = runtime_single(0, 0, 50);
    load_and_run(&mut rt, &ton_pkg("ton"));
    clock.advance_ms(50);
    rt.step().unwrap();
    let et = rt.engine().vm().unwrap().primitives().ton[0].et;
    assert!(et > 0);
    let start = rt.engine().vm().unwrap().primitives().ton[0].start_ms;
    assert!(rt.engine().vm().unwrap().primitives().ton[0].running);

    rt.engine_mut().request_mode(ModeRequest::Stop);
    clock.advance_ms(50);
    rt.step().unwrap();
    assert_eq!(rt.mode(), OperatingMode::Stop);
    assert!(!rt.engine().epoch_hooks().first_scan(0));
    assert_eq!(rt.engine().vm().unwrap().primitives().ton[0].et, et);
    assert_eq!(
        rt.engine().vm().unwrap().primitives().ton[0].start_ms,
        start
    );

    rt.engine_mut().request_mode(ModeRequest::Run);
    clock.advance_ms(50);
    rt.step().unwrap();
    assert_eq!(rt.mode(), OperatingMode::Run);
    assert!(!rt.engine().epoch_hooks().first_scan(0));
    let et_run = rt.engine().vm().unwrap().primitives().ton[0].et;
    assert!(
        et_run >= et,
        "STOP→RUN must resume timers, not cold-reset (et {et} → {et_run})"
    );
}

#[test]
fn validation_failure_never_faults() {
    let (mut rt, _sim, _clock) = runtime_single(0, 0, 50);
    load_and_run(&mut rt, &ton_pkg("ok"));
    assert_eq!(rt.mode(), OperatingMode::Run);
    let err = rt.upload(b"not-a-package").unwrap_err();
    assert!(err.to_string().contains("magic") || err.to_string().contains("package"));
    assert_eq!(rt.mode(), OperatingMode::Run);
    assert_ne!(rt.mode(), OperatingMode::Fault);
    assert_eq!(rt.phase(), ProgramPhase::Idle);
    assert_eq!(rt.engine().current_program_id(), Some("ok"));
}

#[test]
fn bumpless_downgraded_when_hash_differs() {
    let (mut rt, _sim, _clock) = runtime_single(0, 1, 20);
    load_and_run(
        &mut rt,
        &pack(&PackOpts {
            id: "a",
            spasm: Q_TRUE,
            tasks: &[("main", "task.main")],
            retain: &[],
            restart: RestartPolicy::SafeReset,
            q_tags: &[("Q0", 0)],
        }),
    );
    let report = rt
        .upload(&pack(&PackOpts {
            id: "b",
            spasm: Q_FALSE,
            tasks: &[("main", "task.main")],
            retain: &[],
            restart: RestartPolicy::Bumpless,
            q_tags: &[("Q1", 0)], // different %Q tag set → different hash
        }))
        .unwrap();
    assert!(report.bumpless_downgraded);
    assert!(!report.bumpless_honored);
}

#[test]
fn require_signature_rejects_unsigned_without_fault() {
    let sim = SharedSim::new(0, 0);
    let io = ScanIo::new(ProcessImage::with_sizes(0, 0, 0), Box::new(sim));
    let clock = plc_scan::VirtualClock::new();
    let mut rt = plc_runtime::Runtime::new(
        common::single_plan(50),
        io,
        Box::new(clock),
        RuntimeConfig {
            require_signature: true,
            public_keys: Vec::new(),
            force_retain_incompat: false,
        },
    )
    .unwrap();
    let err = rt.upload(&ton_pkg("x")).unwrap_err();
    assert!(
        err.to_string().contains("unsigned") || err.to_string().contains("signature"),
        "{err}"
    );
    assert_eq!(rt.mode(), OperatingMode::Stop);
    assert_ne!(rt.mode(), OperatingMode::Fault);
    assert_eq!(rt.phase(), ProgramPhase::Idle);
}
