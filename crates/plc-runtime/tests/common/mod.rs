//! Package + runtime test helpers.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use plc_io::{DriverDiag, InputUpdate, IoDriver, IoError, OutputImage, PlcValue, ProcessImage};
use plc_io_sim::SimDriver;
use plc_ir::{assemble, IrType};
use plc_package::{
    IrTypeName, Manifest, ManifestRetainSymbol, PackageBuilder, RestartPolicy, TagEntry, TagKind,
};
use plc_runtime::{Runtime, RuntimeConfig};
use plc_scan::{ScanIo, ScanPlan, TaskPlan, VirtualClock};

#[derive(Clone)]
pub struct SharedSim(pub Arc<Mutex<SimDriver>>);

impl SharedSim {
    pub fn new(n_i: usize, n_q: usize) -> Self {
        let mut d = SimDriver::new("sim", n_i, n_q);
        d.start().expect("start");
        Self(Arc::new(Mutex::new(d)))
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

pub fn single_plan(period_ms: u32) -> ScanPlan {
    ScanPlan::new(vec![TaskPlan {
        name: "main".into(),
        period_ms,
        entry: "task.main".into(),
        priority: 50,
    }])
    .unwrap()
}

pub fn multi_plan() -> ScanPlan {
    ScanPlan::new(vec![
        TaskPlan {
            name: "fast".into(),
            period_ms: 20,
            entry: "task.fast".into(),
            priority: 100,
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

pub struct PackOpts<'a> {
    pub id: &'a str,
    pub spasm: &'a str,
    pub tasks: &'a [(&'a str, &'a str)],
    pub retain: &'a [(&'a str, IrType, u32)],
    pub restart: RestartPolicy,
    pub q_tags: &'a [(&'a str, u32)],
}

pub fn pack(opts: &PackOpts<'_>) -> Vec<u8> {
    let module = assemble(opts.spasm).expect("assemble");
    let mut task_entries = BTreeMap::new();
    for (name, symbol) in opts.tasks {
        task_entries.insert((*name).into(), (*symbol).into());
    }
    let retain_symbols = opts
        .retain
        .iter()
        .map(|(name, ty, offset)| ManifestRetainSymbol {
            name: (*name).into(),
            ty: IrTypeName(*ty),
            offset: *offset,
        })
        .collect();
    let tag_dictionary = opts
        .q_tags
        .iter()
        .map(|(name, slot)| TagEntry {
            name: (*name).into(),
            ty: IrTypeName(IrType::Bool),
            kind: TagKind::Q,
            slot: Some(*slot),
        })
        .collect();
    let manifest = Manifest {
        id: opts.id.into(),
        version: "0.1.0".into(),
        build_id: "test".into(),
        ir_major: module.ir_major,
        ir_minor: module.ir_minor,
        primitive_abi: 1,
        task_entries,
        retain_symbols,
        tag_dictionary,
        restart_policy: opts.restart,
        compatibility_hash: "00".repeat(32),
        input_slots: Some(module.input_slots),
        output_slots: Some(module.output_slots),
        data_size: Some(module.data_size),
        retain_size: Some(module.retain_size),
        const_size: Some(module.const_size),
    };
    PackageBuilder::new(manifest)
        .section_module(&module)
        .unwrap()
        .unsigned()
        .to_bytes()
        .unwrap()
}

pub fn runtime_single(
    n_i: usize,
    n_q: usize,
    period_ms: u32,
) -> (Runtime, SharedSim, VirtualClock) {
    let sim = SharedSim::new(n_i, n_q);
    let io = ScanIo::new(ProcessImage::with_sizes(n_i, n_q, 0), Box::new(sim.clone()));
    let clock = VirtualClock::new();
    let rt = Runtime::new(
        single_plan(period_ms),
        io,
        Box::new(clock.clone()),
        RuntimeConfig::default(),
    )
    .unwrap();
    (rt, sim, clock)
}

pub fn runtime_multi() -> (Runtime, SharedSim, VirtualClock) {
    let sim = SharedSim::new(0, 2);
    let io = ScanIo::new(ProcessImage::with_sizes(0, 2, 0), Box::new(sim.clone()));
    let clock = VirtualClock::new();
    let rt = Runtime::new(
        multi_plan(),
        io,
        Box::new(clock.clone()),
        RuntimeConfig::default(),
    )
    .unwrap();
    (rt, sim, clock)
}
