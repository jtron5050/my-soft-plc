//! Flush / load round-trip and A/B slot alternation.

mod common;

use plc_retain::{LoadSource, RetainStore};

#[test]
fn flush_then_load_same_layout() {
    let tmp = common::TempDir::new("round");
    let store = tmp.store();
    let layout = common::simple_layout();
    let img = common::image_with(true, 77);

    let flush = store.flush("prog-a", &layout, &img).unwrap();
    assert_eq!(flush.generation, 1);
    assert_eq!(flush.slot, 0);

    let mut dst = vec![0u8; 8];
    let report = store.load("prog-a", &layout, &mut dst).unwrap();
    assert_eq!(report.source, LoadSource::Slot0);
    assert_eq!(report.generation, 1);
    assert_eq!(report.kept, 2);
    assert!(!report.missing);
    assert!(!report.corrupt);
    assert_eq!(dst, img);
}

#[test]
fn second_flush_uses_other_slot() {
    let tmp = common::TempDir::new("ab");
    let store = tmp.store();
    let layout = common::simple_layout();

    let f1 = store
        .flush("line", &layout, &common::image_with(false, 1))
        .unwrap();
    let f2 = store
        .flush("line", &layout, &common::image_with(true, 2))
        .unwrap();
    assert_eq!(f1.slot, 0);
    assert_eq!(f1.generation, 1);
    assert_eq!(f2.slot, 1);
    assert_eq!(f2.generation, 2);
    assert!(store.slot_file("line", 0).unwrap().exists());
    assert!(store.slot_file("line", 1).unwrap().exists());

    let mut dst = vec![0u8; 8];
    let report = store.load("line", &layout, &mut dst).unwrap();
    assert_eq!(report.source, LoadSource::Slot1);
    assert_eq!(report.generation, 2);
    assert_eq!(dst, common::image_with(true, 2));
}

#[test]
fn flush_creates_dir() {
    let tmp = common::TempDir::new("mkdir");
    let nested = tmp.path.join("nested/retain");
    assert!(!nested.exists());
    let store = RetainStore::open(&nested).unwrap();
    assert!(nested.is_dir());
    let layout = common::simple_layout();
    store
        .flush("x", &layout, &common::image_with(false, 0))
        .unwrap();
    assert!(store.exists("x"));
}

#[test]
fn program_id_rejected() {
    let tmp = common::TempDir::new("id");
    let store = tmp.store();
    let layout = common::simple_layout();
    let img = common::image_with(false, 0);
    for bad in ["", "../x", "a/b", "a\\b", "has space"] {
        let err = store.flush(bad, &layout, &img).unwrap_err();
        assert!(
            err.to_string().contains("invalid retain program id"),
            "{bad}: {err}"
        );
    }
    assert!(store.path_for("../x").is_err());
    assert!(!store.exists("../x"));
}
