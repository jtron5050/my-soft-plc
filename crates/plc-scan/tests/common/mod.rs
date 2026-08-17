//! Shared test helpers for plc-scan integration tests.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use plc_io::{DriverDiag, InputUpdate, IoDriver, IoError, OutputImage, PlcValue, ProcessImage};
use plc_io_sim::SimDriver;
use plc_scan::{ScanEngine, ScanIo, ScanPlan, TaskPlan, VirtualClock};
use plc_types::Quality;
use plc_vm::{Vm, VmConfig};

/// Shared sim driver so tests can inject inputs after the engine owns it.
#[derive(Clone)]
pub struct SharedSim(pub Arc<Mutex<SimDriver>>);

impl SharedSim {
    pub fn new(n_i: usize, n_q: usize) -> Self {
        let mut d = SimDriver::new("sim", n_i, n_q);
        d.start().expect("start");
        Self(Arc::new(Mutex::new(d)))
    }

    pub fn set_input(&self, idx: usize, value: PlcValue) {
        self.0.lock().unwrap().set_input(idx, value);
    }

    pub fn set_quality(&self, idx: usize, q: Quality) {
        self.0.lock().unwrap().set_input_quality(idx, q);
    }

    pub fn last_outputs(&self) -> Vec<PlcValue> {
        self.0.lock().unwrap().last_outputs.clone()
    }

    pub fn last_force_safe(&self) -> bool {
        self.0.lock().unwrap().last_force_safe
    }
}

impl IoDriver for SharedSim {
    fn name(&self) -> &'static str {
        "sim"
    }

    fn start(&mut self) -> Result<(), IoError> {
        Ok(())
    }

    fn stop(&mut self) {}

    fn poll_inputs(&mut self, out: &mut InputUpdate) -> Result<(), IoError> {
        self.0.lock().unwrap().poll_inputs(out)
    }

    fn apply_outputs(&mut self, image: &OutputImage) -> Result<(), IoError> {
        self.0.lock().unwrap().apply_outputs(image)
    }

    fn diagnostics(&self) -> DriverDiag {
        self.0.lock().unwrap().diagnostics()
    }
}

pub fn arith_spasm() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../samples/programs/arith-demo/fixture.spasm"
    );
    std::fs::read_to_string(path).expect("arith-demo fixture")
}

pub fn vm_from_spasm(src: &str) -> Vm {
    Vm::from_spasm(src, &VmConfig::default()).expect("assemble+load")
}

pub fn single_task_plan(period_ms: u32, entry: &str) -> ScanPlan {
    ScanPlan::new(vec![TaskPlan {
        name: "main".into(),
        period_ms,
        entry: entry.into(),
        priority: 50,
    }])
    .unwrap()
}

pub fn multi_rate_plan() -> ScanPlan {
    ScanPlan::new(vec![
        TaskPlan {
            name: "fast".into(),
            period_ms: 20,
            entry: "task.fast".into(),
            priority: 100,
        },
        TaskPlan {
            name: "main".into(),
            period_ms: 50,
            entry: "task.main".into(),
            priority: 50,
        },
        TaskPlan {
            name: "slow".into(),
            period_ms: 500,
            entry: "task.slow".into(),
            priority: 10,
        },
    ])
    .unwrap()
}

/// Q0 := (I0 AND I1) AND I0.quality — arith-demo through sim.
pub fn arith_engine() -> (ScanEngine, SharedSim, VirtualClock) {
    let vm = vm_from_spasm(&arith_spasm());
    let sim = SharedSim::new(2, 1);
    let io = ScanIo::new(ProcessImage::with_sizes(2, 1, 0), Box::new(sim.clone()));
    let clock = VirtualClock::new();
    let engine = ScanEngine::new(
        single_task_plan(50, "task.main"),
        io,
        Some(vm),
        Box::new(clock.clone()),
    )
    .expect("engine");
    (engine, sim, clock)
}

/// Multi-rate program: each task writes a distinct %Q from I0 or a constant.
pub fn multi_spasm() -> &'static str {
    r#"
.header data_size=16 retain_size=0 input_slots=1 output_slots=3
.entry task.fast
LD_I 0
ST_Q 0
HALT
.entry task.main
PUSHI_BOOL 1
ST_Q 1
HALT
.entry task.slow
PUSHI_BOOL 1
ST_Q 2
HALT
"#
}

pub fn multi_engine() -> (ScanEngine, SharedSim, VirtualClock) {
    let vm = vm_from_spasm(multi_spasm());
    let sim = SharedSim::new(1, 3);
    let io = ScanIo::new(ProcessImage::with_sizes(1, 3, 0), Box::new(sim.clone()));
    let clock = VirtualClock::new();
    let engine =
        ScanEngine::new(multi_rate_plan(), io, Some(vm), Box::new(clock.clone())).expect("engine");
    (engine, sim, clock)
}

pub fn halt_only_spasm() -> &'static str {
    r#"
.header data_size=8 retain_size=0 input_slots=0 output_slots=1
.entry task.main
PUSHI_BOOL 1
ST_Q 0
HALT
"#
}
