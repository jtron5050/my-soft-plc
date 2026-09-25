//! Compile sample projects and check VM / package behavior.

use std::path::PathBuf;

use plc_compiler::{compile_project, CompileOptions};
use plc_ir::verify_module;
use plc_package::{validate, VerifyPolicy};
use plc_vm::{ExecResult, Vm, VmConfig, VmValue};

fn sample_project(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/programs")
        .join(name)
        .join("project.toml")
}

#[test]
fn compile_arith_demo_quality_gate() {
    let out = compile_project(&sample_project("arith-demo"), &CompileOptions::default()).unwrap();
    verify_module(&out.module).unwrap();
    let mut vm = Vm::load(out.module.clone(), &VmConfig::default()).unwrap();

    vm.inputs_mut().set(0, VmValue::Bool(true), 0).unwrap();
    vm.inputs_mut().set(1, VmValue::Bool(true), 0).unwrap();
    vm.inputs_mut().set_quality_good(0, true, 0).unwrap();
    assert_eq!(vm.run_entry("task.main", 0).unwrap(), ExecResult::Halted);
    assert!(vm.outputs().get(0, 0).unwrap().as_bool());

    vm.inputs_mut().set_quality_good(0, false, 0).unwrap();
    assert_eq!(vm.run_entry("task.main", 0).unwrap(), ExecResult::Halted);
    assert!(!vm.outputs().get(0, 0).unwrap().as_bool());
}

#[test]
fn compile_ton_call_expires() {
    let out = compile_project(&sample_project("ton-call"), &CompileOptions::default()).unwrap();
    let mut vm = Vm::load(out.module, &VmConfig::default()).unwrap();

    // Find q_slot / et_slot — first two data BOOL/TIME after prim shadows.
    // Layout: ton outputs Q@?, ET@? then q_slot, et_slot.
    // Read via running and checking that Q becomes true at t=1000.
    assert_eq!(vm.run_entry("task.main", 0).unwrap(), ExecResult::Halted);
    // After first call ET should be 0 and Q false — scan data for TIME 0 and BOOL false
    // Use known layout: prim outs allocated first (Q bool align1, ET time align4)
    // Actually BOOL at 0, TIME at 4, then q_slot BOOL, et_slot TIME.
    assert!(!vm.data().load(0, 0).unwrap().as_bool()); // ton.Q stash
    assert_eq!(vm.data().load(4, 0).unwrap(), VmValue::Time(0));

    assert_eq!(vm.run_entry("task.main", 500).unwrap(), ExecResult::Halted);
    assert!(!vm.data().load(0, 0).unwrap().as_bool());
    assert_eq!(vm.data().load(4, 0).unwrap(), VmValue::Time(500));

    assert_eq!(vm.run_entry("task.main", 1000).unwrap(), ExecResult::Halted);
    assert!(vm.data().load(0, 0).unwrap().as_bool());
    assert_eq!(vm.data().load(4, 0).unwrap(), VmValue::Time(1000));
}

#[test]
fn compile_rs_latch_user_fb() {
    let out = compile_project(&sample_project("rs-latch"), &CompileOptions::default()).unwrap();
    let module = out.module.clone();
    let mut vm = Vm::load(out.module, &VmConfig::default()).unwrap();

    // s_in, r_in, q_out are data vars; latch instance fields S,R,Q
    // Drive via program: set s_in/r_in then run task.main
    // Find offsets by names order: latch instance then s_in, r_in, q_out
    // Simpler: run FB entry directly like the spasm fixture.
    let fb = module
        .entries
        .iter()
        .find(|e| e.is_user_fb)
        .expect("user fb entry");
    // Instance at base 0: S@0 R@1 Q@2 for elementary BOOL packing
    // But program also allocates — FB body uses relative offsets.
    // Run with data_base 0 by invoking FB entry (Vm sets base from CALL; direct run_entry uses 0).
    vm.data_mut().store(0, VmValue::Bool(true), 0).unwrap(); // S
    vm.data_mut().store(1, VmValue::Bool(false), 0).unwrap(); // R
    vm.data_mut().store(2, VmValue::Bool(false), 0).unwrap(); // Q
    assert_eq!(vm.run_entry(&fb.name, 0).unwrap(), ExecResult::Returned);
    assert!(vm.data().load(2, 0).unwrap().as_bool());
}

#[test]
fn compile_demo_conveyor_package_validates() {
    let out =
        compile_project(&sample_project("demo-conveyor"), &CompileOptions::default()).unwrap();
    verify_module(&out.module).unwrap();
    validate(&out.spkg, VerifyPolicy::unsigned()).unwrap();
    assert!(out.manifest.task_entries.contains_key("fast"));
    assert!(out.manifest.task_entries.contains_key("main"));
    assert!(out.manifest.task_entries.contains_key("slow"));
    assert_eq!(out.module.input_slots, 6);
    assert_eq!(out.module.output_slots, 3);
}

#[test]
fn compile_demo_conveyor_interlock_ready() {
    let out =
        compile_project(&sample_project("demo-conveyor"), &CompileOptions::default()).unwrap();
    let mut vm = Vm::load(out.module, &VmConfig::default()).unwrap();
    // All interlocks OK, not local
    for i in 2..=4 {
        vm.inputs_mut().set(i, VmValue::Bool(i != 4), 0).unwrap(); // chute blocked false
        vm.inputs_mut().set_quality_good(i, true, 0).unwrap();
    }
    vm.inputs_mut().set(4, VmValue::Bool(false), 0).unwrap();
    vm.inputs_mut().set(5, VmValue::Bool(false), 0).unwrap();
    assert_eq!(vm.run_entry("task.fast", 0).unwrap(), ExecResult::Halted);
    assert!(vm.outputs().get(2, 0).unwrap().as_bool()); // Ready
    assert!(!vm.outputs().get(1, 0).unwrap().as_bool()); // Fault
}
