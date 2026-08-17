//! Corruption recovery: fall back to the previous slot, or cold-start.

mod common;

use plc_retain::LoadSource;

#[test]
fn corrupt_active_falls_back() {
    let tmp = common::TempDir::new("fallback");
    let store = tmp.store();
    let layout = common::simple_layout();
    let first = common::image_with(true, 11);
    let second = common::image_with(false, 22);

    store.flush("p", &layout, &first).unwrap();
    store.flush("p", &layout, &second).unwrap();

    let active = store.slot_file("p", 1).unwrap();
    common::corrupt_crc(&active);

    let mut dst = vec![0u8; 8];
    let report = store.load("p", &layout, &mut dst).unwrap();
    assert_eq!(report.source, LoadSource::Slot0);
    assert_eq!(report.generation, 1);
    assert!(report.corrupt);
    assert_eq!(dst, first);
}

#[test]
fn both_slots_corrupt_is_cold() {
    let tmp = common::TempDir::new("both");
    let store = tmp.store();
    let layout = common::simple_layout();
    store
        .flush("p", &layout, &common::image_with(true, 1))
        .unwrap();
    store
        .flush("p", &layout, &common::image_with(true, 2))
        .unwrap();
    common::corrupt_crc(&store.slot_file("p", 0).unwrap());
    common::corrupt_crc(&store.slot_file("p", 1).unwrap());

    let mut dst = vec![0xAAu8; 8];
    let report = store.load("p", &layout, &mut dst).unwrap();
    assert_eq!(report.source, LoadSource::Cold);
    assert!(report.corrupt);
    assert!(!report.missing);
    assert_eq!(dst, vec![0u8; 8]);
}

#[test]
fn missing_store_is_cold() {
    let tmp = common::TempDir::new("missing");
    let store = tmp.store();
    let layout = common::simple_layout();
    let mut dst = vec![0xAAu8; 8];
    let report = store.load("never-written", &layout, &mut dst).unwrap();
    assert_eq!(report.source, LoadSource::Cold);
    assert!(report.missing);
    assert!(!report.corrupt);
    assert_eq!(dst, vec![0u8; 8]);
}
