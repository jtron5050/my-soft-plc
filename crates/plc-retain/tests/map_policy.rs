//! Hot-swap retain policy (keep / drop / new / incompat).

use plc_ir::{IrType, RetainLayout, RetainSymbol};
use plc_retain::{map_retain, RetainError};

fn layout(syms: Vec<RetainSymbol>, size: u32) -> RetainLayout {
    RetainLayout::new(size, syms).unwrap()
}

#[test]
fn map_keep_drop_new() {
    let old = layout(
        vec![
            RetainSymbol::new("keep", IrType::Dint, 0),
            RetainSymbol::new("gone", IrType::Bool, 4),
        ],
        8,
    );
    let new = layout(
        vec![
            RetainSymbol::new("keep", IrType::Dint, 4),
            RetainSymbol::new("fresh", IrType::Int, 0),
        ],
        8,
    );
    let mut old_img = vec![0u8; 8];
    old_img[0..4].copy_from_slice(&99i32.to_le_bytes());
    old_img[4] = 1;
    let mapped = map_retain(&old, &old_img, &new, false).unwrap();
    assert_eq!(&mapped.image[4..8], &99i32.to_le_bytes());
    assert_eq!(&mapped.image[0..2], &[0, 0]);
    assert_eq!(mapped.report.kept, 1);
    assert_eq!(mapped.report.dropped, 1);
    assert_eq!(mapped.report.cold_defaults, 1);
}

#[test]
fn map_type_mismatch_rejects() {
    let old = layout(vec![RetainSymbol::new("x", IrType::Bool, 0)], 4);
    let new = layout(vec![RetainSymbol::new("x", IrType::Dint, 0)], 4);
    let err = map_retain(&old, &[1, 0, 0, 0], &new, false).unwrap_err();
    match err {
        RetainError::Incompatible { names } => assert_eq!(names, ["x".to_string()]),
        other => panic!("unexpected {other}"),
    }
}

#[test]
fn map_type_mismatch_force_zeros() {
    let old = layout(vec![RetainSymbol::new("x", IrType::Dint, 0)], 4);
    let new = layout(vec![RetainSymbol::new("x", IrType::Int, 0)], 4);
    let mut old_img = vec![0u8; 4];
    old_img.copy_from_slice(&(-1i32).to_le_bytes());
    let mapped = map_retain(&old, &old_img, &new, true).unwrap();
    assert_eq!(&mapped.image[..2], &[0, 0]);
    assert_eq!(mapped.report.zeroed_incompat, ["x".to_string()]);
    assert_eq!(mapped.report.kept, 0);
    assert_eq!(mapped.report.cold_defaults, 0);
}
