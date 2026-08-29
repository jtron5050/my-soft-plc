//! Scan-engine helpers for telemetry integration tests.

#![allow(dead_code)]

use plc_io::ProcessImage;
use plc_io_sim::SimDriver;
use plc_scan::{ScanEngine, ScanHandle, ScanIo, ScanPlan, TaskPlan, TelemetrySource, VirtualClock};
use plc_vm::{Vm, VmConfig};

pub fn arith_spasm() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../samples/programs/arith-demo/fixture.spasm"
    );
    std::fs::read_to_string(path).expect("arith-demo fixture")
}

pub fn tiny_engine() -> (ScanEngine, TelemetrySource, ScanHandle, VirtualClock) {
    let vm = Vm::from_spasm(&arith_spasm(), &VmConfig::default()).expect("assemble");
    let sim = SimDriver::new("sim", 2, 1);
    let io = ScanIo::new(ProcessImage::with_sizes(2, 1, 0), Box::new(sim));
    let clock = VirtualClock::new();
    let mut plan = ScanPlan::new(vec![TaskPlan {
        name: "main".into(),
        period_ms: 50,
        entry: "task.main".into(),
        priority: 50,
    }])
    .unwrap();
    plan.telemetry_capacity = 4;
    plan.digital_cos_ms = 0;
    plan.analog_period_ms = 1;
    let engine = ScanEngine::new(plan, io, Some(vm), Box::new(clock.clone())).unwrap();
    let src = engine.telemetry_source();
    let handle = engine.handle();
    (engine, src, handle, clock)
}
