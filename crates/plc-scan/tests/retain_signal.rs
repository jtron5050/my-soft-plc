//! Dirty-retain signal coalesces ST_RETAIN writes.

mod common;

use plc_io::ProcessImage;
use plc_scan::{ModeRequest, ScanEngine, ScanIo};

const RETAIN_SPASM: &str = r#"
.header data_size=8 retain_size=8 input_slots=0 output_slots=0
.entry task.main
PUSHI_BOOL 1
ST_RETAIN 0
HALT
"#;

#[test]
fn st_retain_notifies_and_coalesces() {
    let vm = common::vm_from_spasm(RETAIN_SPASM);
    let sim = common::SharedSim::new(0, 0);
    let io = ScanIo::new(ProcessImage::with_sizes(0, 0, 0), Box::new(sim));
    let clock = plc_scan::VirtualClock::new();
    let mut engine = ScanEngine::new(
        common::single_task_plan(50, "task.main"),
        io,
        Some(vm),
        Box::new(clock.clone()),
    )
    .unwrap();
    let watch = engine.retain_dirty();
    assert!(watch.take().is_none());

    engine.request_mode(ModeRequest::Run);
    engine.step().unwrap();
    let first = watch.take().expect("first ST_RETAIN");
    assert!(first.seq >= 1);
    assert!(watch.take().is_none());

    clock.advance_ms(50);
    engine.step().unwrap();
    clock.advance_ms(50);
    engine.step().unwrap();
    let second = watch.take().expect("coalesced");
    assert!(second.seq > first.seq);
    assert!(watch.take().is_none());
}
