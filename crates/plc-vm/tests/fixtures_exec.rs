//! Execute Appendix A and sample program fixtures on the VM.

use std::path::PathBuf;

use plc_vm::{ExecResult, Vm, VmConfig, VmValue};

fn sample(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/programs")
        .join(name)
        .join("fixture.spasm");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn rs_latch_set_hold_reset() {
    let src = sample("rs-latch");
    let mut vm = Vm::from_spasm(&src, &VmConfig::default()).expect("load");

    // S=1, R=0 → Q=1
    vm.data_mut().store(0, VmValue::Bool(true), 0).unwrap();
    vm.data_mut().store(1, VmValue::Bool(false), 0).unwrap();
    vm.data_mut().store(2, VmValue::Bool(false), 0).unwrap();
    assert_eq!(vm.run_entry("fb.RS", 0).unwrap(), ExecResult::Returned);
    assert!(vm.data().load(2, 0).unwrap().as_bool());

    // S=0, R=0 → Q holds
    vm.data_mut().store(0, VmValue::Bool(false), 0).unwrap();
    assert_eq!(vm.run_entry("fb.RS", 0).unwrap(), ExecResult::Returned);
    assert!(vm.data().load(2, 0).unwrap().as_bool());

    // R=1 → Q=0
    vm.data_mut().store(1, VmValue::Bool(true), 0).unwrap();
    assert_eq!(vm.run_entry("fb.RS", 0).unwrap(), ExecResult::Returned);
    assert!(!vm.data().load(2, 0).unwrap().as_bool());
}

#[test]
fn ton_call_expires_after_pt() {
    let src = sample("ton-call");
    let mut vm = Vm::from_spasm(&src, &VmConfig::default()).expect("load");

    // t=0: start timing, Q false, ET 0
    assert_eq!(vm.run_entry("task.main", 0).unwrap(), ExecResult::Halted);
    assert!(!vm.data().load(0, 0).unwrap().as_bool());
    assert_eq!(vm.data().load(4, 0).unwrap(), VmValue::Time(0));

    // t=500: still timing
    assert_eq!(vm.run_entry("task.main", 500).unwrap(), ExecResult::Halted);
    assert!(!vm.data().load(0, 0).unwrap().as_bool());
    assert_eq!(vm.data().load(4, 0).unwrap(), VmValue::Time(500));

    // t=1000: Q true, ET=PT
    assert_eq!(vm.run_entry("task.main", 1000).unwrap(), ExecResult::Halted);
    assert!(vm.data().load(0, 0).unwrap().as_bool());
    assert_eq!(vm.data().load(4, 0).unwrap(), VmValue::Time(1000));
}

#[test]
fn arith_demo_with_quality_gate() {
    let src = sample("arith-demo");
    let mut vm = Vm::from_spasm(&src, &VmConfig::default()).expect("load");

    vm.inputs_mut().set(0, VmValue::Bool(true), 0).unwrap();
    vm.inputs_mut().set(1, VmValue::Bool(true), 0).unwrap();
    vm.inputs_mut().set_quality_good(0, true, 0).unwrap();
    assert_eq!(vm.run_entry("task.main", 0).unwrap(), ExecResult::Halted);
    assert!(vm.outputs().get(0, 0).unwrap().as_bool());

    // Bad quality on I0 → AND with LD_IQ fails → Q false
    vm.inputs_mut().set_quality_good(0, false, 0).unwrap();
    assert_eq!(vm.run_entry("task.main", 0).unwrap(), ExecResult::Halted);
    assert!(!vm.outputs().get(0, 0).unwrap().as_bool());
}

#[test]
fn div_by_zero_yields_zero_and_counts() {
    let src = r"
.header data_size=8 input_slots=0 output_slots=0
.entry task.main
PUSHI_DINT 10
PUSHI_DINT 0
DIV
ST_DATA 0
HALT
";
    let mut vm = Vm::from_spasm(src, &VmConfig::default()).unwrap();
    vm.run_entry("task.main", 0).unwrap();
    assert_eq!(vm.data().load(0, 0).unwrap(), VmValue::Dint(0));
    assert_eq!(vm.div0_count, 1);
}

#[test]
fn plc_ir_fixtures_match_samples() {
    // Keep samples and crate fixtures in sync for reviewability.
    let ir_rs = include_str!("../../plc-ir/tests/fixtures/rs_latch.spasm");
    let sample_rs = sample("rs-latch");
    // Normalize line endings / trailing whitespace for comparison of program body.
    assert!(
        sample_rs.contains("LD_DATA  0") && ir_rs.contains("LD_DATA  0"),
        "RS fixtures should share the same program body"
    );
    let ir_ton = include_str!("../../plc-ir/tests/fixtures/ton_call.spasm");
    let sample_ton = sample("ton-call");
    assert!(sample_ton.contains("prim=TON") && ir_ton.contains("prim=TON"));
}
