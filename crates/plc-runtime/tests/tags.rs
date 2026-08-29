//! Tag dictionary debug read and %Q force overlay.

mod common;

use plc_io::PlcValue;
use plc_package::TagKind;
use plc_runtime::RuntimeError;

use common::{pack, runtime_single, PackOpts};

const Q_WRITE: &str = r#"
.header data_size=8 retain_size=0 input_slots=1 output_slots=1
.entry task.main
PUSHI_BOOL 1
ST_Q       0
HALT
"#;

#[test]
fn read_image_meta_and_force_q() {
    let (mut rt, _sim, _clock) = runtime_single(1, 1, 50);
    let names = rt.tag_names();
    assert!(names.iter().any(|t| t.name == "Q0" && t.kind == TagKind::Q));

    let view = rt.read_tag("Q0").unwrap();
    assert_eq!(view.kind, TagKind::Q);
    assert!(!view.forced);

    rt.force_tag("Q0", PlcValue::Bool(true)).unwrap();
    let view = rt.read_tag("Q0").unwrap();
    assert!(view.forced);
    assert_eq!(view.value, PlcValue::Bool(true));

    rt.clear_force("Q0").unwrap();
    assert!(!rt.read_tag("Q0").unwrap().forced);
}

#[test]
fn force_rejects_inputs() {
    let (mut rt, _sim, _clock) = runtime_single(1, 1, 50);
    let err = rt.force_tag("I0", PlcValue::Bool(true)).unwrap_err();
    assert!(matches!(err, RuntimeError::BadRequest(_)));
}

#[test]
fn unknown_tag_is_not_found() {
    let (rt, _sim, _clock) = runtime_single(1, 1, 50);
    let err = rt.read_tag("nope").unwrap_err();
    assert!(matches!(err, RuntimeError::NotFound(_)));
}

#[test]
fn dictionary_names_win_after_arm() {
    let (mut rt, _sim, _clock) = runtime_single(1, 1, 50);
    let pkg = pack(&PackOpts {
        id: "line",
        spasm: Q_WRITE,
        tasks: &[("main", "task.main")],
        retain: &[],
        restart: plc_package::RestartPolicy::SafeReset,
        q_tags: &[("Conveyor1/RunFwd", 0)],
    });
    rt.upload(&pkg).unwrap();
    let names = rt.tag_names();
    assert!(names.iter().any(|t| t.name == "Conveyor1/RunFwd"));
    rt.force_tag("Conveyor1/RunFwd", PlcValue::Bool(true))
        .unwrap();
    assert!(rt.read_tag("Conveyor1/RunFwd").unwrap().forced);
}

#[test]
fn prepare_arm_does_not_need_engine_lock() {
    let (mut rt, _sim, _clock) = runtime_single(1, 1, 50);
    let pkg = pack(&PackOpts {
        id: "line",
        spasm: Q_WRITE,
        tasks: &[("main", "task.main")],
        retain: &[],
        restart: plc_package::RestartPolicy::SafeReset,
        q_tags: &[],
    });
    let ctx = rt.begin_arm().unwrap();
    let prepared = plc_runtime::Runtime::prepare_arm(&pkg, &ctx).unwrap();
    // Scan-side metadata still validating; engine is not yet armed.
    assert_eq!(rt.phase(), plc_types::ProgramPhase::Validating);
    assert!(rt.engine().armed_program_id().is_none());
    rt.commit_arm(prepared).unwrap();
    assert_eq!(rt.phase(), plc_types::ProgramPhase::Armed);
}

#[test]
fn begin_arm_refuses_activate_pending() {
    let (mut rt, _sim, _clock) = runtime_single(1, 1, 50);
    let pkg = pack(&PackOpts {
        id: "line",
        spasm: Q_WRITE,
        tasks: &[("main", "task.main")],
        retain: &[],
        restart: plc_package::RestartPolicy::SafeReset,
        q_tags: &[],
    });
    rt.upload(&pkg).unwrap();
    rt.activate().unwrap();
    let err = rt.begin_arm().unwrap_err();
    assert!(matches!(err, RuntimeError::Conflict { context } if context.contains("activate")));
}

#[test]
fn abort_arm_does_not_clobber_swapping() {
    let (mut rt, _sim, _clock) = runtime_single(1, 1, 50);
    let _ctx = rt.begin_arm().unwrap();
    assert_eq!(rt.phase(), plc_types::ProgramPhase::Validating);
    rt.engine()
        .epoch_hooks()
        .set_phase(plc_types::ProgramPhase::Swapping);
    rt.abort_arm();
    assert_eq!(rt.phase(), plc_types::ProgramPhase::Swapping);
}
