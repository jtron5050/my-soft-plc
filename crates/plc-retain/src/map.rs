//! Symbol-path remap (keep / cold / drop / incompat).

use plc_ir::RetainLayout;

use crate::codec::{records_from_image, RetainRecord};
use crate::error::RetainError;

/// Outcome of [`map_retain`] / [`apply_records`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MapReport {
    /// Same path + compatible type, value copied.
    pub kept: u32,
    /// Present only in the new layout (left at zero).
    pub cold_defaults: u32,
    /// Present only in the old image (dropped).
    pub dropped: u32,
    /// Same path, different type, zeroed because `force_incompat` was set.
    pub zeroed_incompat: Vec<String>,
}

/// New retain image plus the remap report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedRetain {
    /// Packed new-layout image (`new.retain_size` bytes, zeros + kept values).
    pub image: Vec<u8>,
    /// Keep / drop / cold / zeroed-incompat counts.
    pub report: MapReport,
}

/// Arm-time remap: same path + type → keep; new → cold; missing → drop.
///
/// Incompatible type → [`RetainError::Incompatible`] unless `force_incompat`,
/// in which case those slots are zeroed and named in
/// [`MapReport::zeroed_incompat`].
///
/// PR-10 calls this on the non-RT arm path to build the shadow retain image.
/// The activate critical section only pointer-swings and `memcpy`s that image.
pub fn map_retain(
    old: &RetainLayout,
    old_image: &[u8],
    new: &RetainLayout,
    force_incompat: bool,
) -> Result<MappedRetain, RetainError> {
    let records = records_from_image(old, old_image)?;
    apply_records(&records, new, force_incompat)
}

/// Apply decoded NV records onto `layout` (offsets are a VM concern).
pub fn apply_records(
    records: &[RetainRecord],
    layout: &RetainLayout,
    force_incompat: bool,
) -> Result<MappedRetain, RetainError> {
    let mut image = vec![0u8; layout.retain_size as usize];
    let mut report = MapReport::default();
    let mut incompat = Vec::new();
    let mut used = vec![false; layout.symbols.len()];

    for rec in records {
        match layout.get(&rec.name) {
            None => report.dropped += 1,
            Some(sym) if sym.ty == rec.ty => {
                let start = sym.offset as usize;
                let width = rec.ty.byte_width();
                if rec.value.len() != width {
                    return Err(RetainError::codec(format!(
                        "record {} value width {} != {width}",
                        rec.name,
                        rec.value.len()
                    )));
                }
                image[start..start + width].copy_from_slice(&rec.value);
                report.kept += 1;
                if let Ok(i) = layout
                    .symbols
                    .binary_search_by(|s| s.name.as_str().cmp(&rec.name))
                {
                    used[i] = true;
                }
            }
            Some(_) => incompat.push(rec.name.clone()),
        }
    }

    if !incompat.is_empty() && !force_incompat {
        return Err(RetainError::Incompatible { names: incompat });
    }
    if force_incompat {
        report.zeroed_incompat = incompat;
    }

    for used in used {
        if !used {
            report.cold_defaults += 1;
        }
    }
    // Incompat names exist in the new layout but were not copied; they were
    // counted as cold above. Subtract them so cold_defaults is "new symbols
    // only" when force-zeroing mismatches.
    report.cold_defaults = report
        .cold_defaults
        .saturating_sub(report.zeroed_incompat.len() as u32);

    Ok(MappedRetain { image, report })
}

#[cfg(test)]
mod tests {
    use plc_ir::{IrType, RetainLayout, RetainSymbol};

    use super::*;

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
            RetainError::Incompatible { names } => assert_eq!(names, vec!["x".to_string()]),
            other => panic!("unexpected {other}"),
        }
    }

    #[test]
    fn map_type_mismatch_force_zeros() {
        let old = layout(vec![RetainSymbol::new("x", IrType::Dint, 0)], 4);
        let new = layout(vec![RetainSymbol::new("x", IrType::Int, 0)], 4);
        let mut old_img = vec![0u8; 4];
        old_img[0..4].copy_from_slice(&(-1i32).to_le_bytes());
        let mapped = map_retain(&old, &old_img, &new, true).unwrap();
        assert_eq!(&mapped.image[..2], &[0, 0]);
        assert_eq!(mapped.report.zeroed_incompat, vec!["x".to_string()]);
        assert_eq!(mapped.report.kept, 0);
        assert_eq!(mapped.report.cold_defaults, 0);
    }
}
