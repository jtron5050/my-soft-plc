//! Demo conveyor fixture: VirtualClock sequence (no HTTP).

mod common;

use std::sync::Arc;

use plc_io::{IoMap, PlcValue, ProcessImage};
use plc_io_sim::SharedSim;
use plc_runtime::{Runtime, RuntimeConfig};
use plc_scan::{ModeRequest, ScanIo};
use plc_types::{OperatingMode, Quality};

use common::{demo_plan, pack_demo_conveyor, pack_demo_from_spasm};

const SPASM: &str = include_str!("../../../samples/programs/demo-conveyor/fixture.spasm");
fn spkg_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/programs/demo-conveyor/fixture.spkg")
}

fn demo_runtime() -> (Runtime, SharedSim, plc_scan::VirtualClock) {
    let map_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/configs/sim-plant-io-map.yaml");
    let image = IoMap::load_from_path(&map_path)
        .expect("io-map")
        .build_image()
        .expect("image");
    assert_eq!(image.inputs.len(), 6);
    assert_eq!(image.outputs.len(), 3);
    let sim = SharedSim::new("sim", image.inputs.len(), image.outputs.len());
    let io = ScanIo::new(image, Box::new(sim.clone()));
    let clock = plc_scan::VirtualClock::new();
    let mut rt = Runtime::new(
        demo_plan(),
        io,
        Box::new(clock.clone()),
        RuntimeConfig::default(),
    )
    .unwrap();
    rt.set_input_injector(Arc::new(sim.clone()));
    (rt, sim, clock)
}

fn q_bool(rt: &Runtime, name: &str) -> bool {
    rt.read_tag(name).unwrap().value.as_bool()
}

fn bring_up_sim(rt: &mut Runtime, clock: &plc_scan::VirtualClock) {
    let pkg = pack_demo_from_spasm(SPASM);
    rt.upload(&pkg).unwrap();
    rt.activate().unwrap();
    rt.engine_mut().request_mode(ModeRequest::Sim);
    rt.step().unwrap();
    assert_eq!(rt.mode(), OperatingMode::Sim);
    let _ = clock;
}

fn set_i(rt: &mut Runtime, name: &str, v: bool) {
    rt.inject_input(name, PlcValue::Bool(v)).unwrap();
}

fn run_ms(rt: &mut Runtime, clock: &plc_scan::VirtualClock, ms: u64) {
    let mut left = ms;
    while left > 0 {
        let step = left.min(20);
        clock.advance_ms(step);
        rt.run_due().unwrap();
        left -= step;
    }
}

#[test]
fn demo_conveyor_spasm_assembles() {
    plc_ir::assemble(SPASM).expect("assemble");
}

#[test]
fn demo_conveyor_spkg_matches_spasm() {
    let got = pack_demo_from_spasm(SPASM);
    let path = spkg_path();
    if std::env::var("UPDATE_FIXTURES").is_ok() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).expect("mkdir");
        }
        std::fs::write(&path, &got).expect("write fixture.spkg");
    }
    let want = std::fs::read(&path).unwrap_or_default();
    assert_eq!(
        got, want,
        "fixture.spkg stale; run UPDATE_FIXTURES=1 cargo test -p plc-runtime demo_conveyor_spkg"
    );
    let _ = pack_demo_conveyor();
}

#[test]
fn permissives_ready_then_start_ton_then_stop() {
    let (mut rt, _sim, clock) = demo_runtime();
    bring_up_sim(&mut rt, &clock);

    set_i(&mut rt, "Conveyor1/PullCordOK", true);
    set_i(&mut rt, "Conveyor1/BeltSlipOK", true);
    set_i(&mut rt, "Conveyor1/ChuteBlocked", false);
    set_i(&mut rt, "Conveyor1/LocalMode", false);
    run_ms(&mut rt, &clock, 20);
    assert!(q_bool(&rt, "Conveyor1/Ready"), "ready");
    assert!(!q_bool(&rt, "Conveyor1/Fault"));
    assert!(!q_bool(&rt, "Conveyor1/RunFwd"));

    set_i(&mut rt, "Conveyor1/StartCmd", true);
    run_ms(&mut rt, &clock, 40);
    assert!(
        !q_bool(&rt, "Conveyor1/RunFwd"),
        "TON has not elapsed yet (ready={} fault={})",
        q_bool(&rt, "Conveyor1/Ready"),
        q_bool(&rt, "Conveyor1/Fault")
    );
    run_ms(&mut rt, &clock, 1200);
    assert!(
        q_bool(&rt, "Conveyor1/RunFwd"),
        "should run after 1s TON (ready={} fault={})",
        q_bool(&rt, "Conveyor1/Ready"),
        q_bool(&rt, "Conveyor1/Fault")
    );

    set_i(&mut rt, "Conveyor1/StopCmd", true);
    run_ms(&mut rt, &clock, 50);
    assert!(!q_bool(&rt, "Conveyor1/RunFwd"));
}

#[test]
fn chute_blocked_faults_and_drops_run() {
    let (mut rt, _sim, clock) = demo_runtime();
    bring_up_sim(&mut rt, &clock);
    set_i(&mut rt, "Conveyor1/PullCordOK", true);
    set_i(&mut rt, "Conveyor1/BeltSlipOK", true);
    set_i(&mut rt, "Conveyor1/StartCmd", true);
    run_ms(&mut rt, &clock, 1100);
    assert!(q_bool(&rt, "Conveyor1/RunFwd"));

    set_i(&mut rt, "Conveyor1/ChuteBlocked", true);
    run_ms(&mut rt, &clock, 20);
    assert!(q_bool(&rt, "Conveyor1/Fault"));
    assert!(!q_bool(&rt, "Conveyor1/Ready"));
    run_ms(&mut rt, &clock, 50);
    assert!(!q_bool(&rt, "Conveyor1/RunFwd"));
}

#[test]
fn bad_quality_on_pullcord_faults() {
    let (mut rt, _sim, clock) = demo_runtime();
    bring_up_sim(&mut rt, &clock);
    set_i(&mut rt, "Conveyor1/PullCordOK", true);
    set_i(&mut rt, "Conveyor1/BeltSlipOK", true);
    run_ms(&mut rt, &clock, 20);
    assert!(q_bool(&rt, "Conveyor1/Ready"));

    rt.inject_input_quality("Conveyor1/PullCordOK", Quality::Bad)
        .unwrap();
    run_ms(&mut rt, &clock, 20);
    assert!(q_bool(&rt, "Conveyor1/Fault"));
    assert!(!q_bool(&rt, "Conveyor1/Ready"));
}

#[test]
fn run_hours_increment_on_slow_task() {
    let (mut rt, _sim, clock) = demo_runtime();
    bring_up_sim(&mut rt, &clock);
    set_i(&mut rt, "Conveyor1/PullCordOK", true);
    set_i(&mut rt, "Conveyor1/BeltSlipOK", true);
    set_i(&mut rt, "Conveyor1/StartCmd", true);
    run_ms(&mut rt, &clock, 1100);
    assert!(q_bool(&rt, "Conveyor1/RunFwd"));
    run_ms(&mut rt, &clock, 1500);
    let hours = rt.engine().vm().unwrap().retain().as_bytes();
    let bits = u32::from_le_bytes(hours[0..4].try_into().unwrap());
    let v = f32::from_bits(bits);
    assert!(v > 0.0, "RunHours should accumulate, got {v}");
}

#[test]
fn io_map_image_matches_package_slots() {
    let map = IoMap::load_from_path(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../samples/configs/sim-plant-io-map.yaml"),
    )
    .unwrap();
    let image: ProcessImage = map.build_image().unwrap();
    let module = plc_ir::assemble(SPASM).unwrap();
    assert_eq!(image.inputs.len() as u32, module.input_slots);
    assert_eq!(image.outputs.len() as u32, module.output_slots);
}
