//! Sparkplug device catalog from the program tag dictionary (PR-14).

use plc_io::ProcessImage;
use plc_ir::IrType;
use plc_package::{TagEntry, TagKind};
use plc_telemetry::{CatalogTag, MetricType, TagCatalog};

use crate::error::RuntimeError;

/// Map `%I`/`%Q` dictionary entries to Sparkplug device metrics.
///
/// `unit` is taken from io-map slot metadata when `image` is provided.
pub fn catalog_from_tags(
    tags: &[TagEntry],
    image: Option<&ProcessImage>,
) -> Result<TagCatalog, RuntimeError> {
    let mut catalog = Vec::new();
    for t in tags {
        let is_input = match t.kind {
            TagKind::I => true,
            TagKind::Q => false,
            TagKind::M | TagKind::R | TagKind::Internal => continue,
        };
        let slot = t.slot.ok_or_else(|| {
            RuntimeError::bad_request(format!("tag '{}' has no image slot", t.name))
        })?;
        let unit = unit_for(image, is_input, slot);
        catalog.push(CatalogTag {
            name: t.name.clone(),
            value_type: ir_metric(t.ty.0),
            is_input,
            slot,
            unit,
        });
    }
    TagCatalog::from_tags(catalog).map_err(|e| RuntimeError::bad_request(e.to_string()))
}

fn unit_for(image: Option<&ProcessImage>, is_input: bool, slot: u32) -> String {
    let Some(image) = image else {
        return String::new();
    };
    let meta = if is_input {
        image.input_meta.get(slot as usize)
    } else {
        image.output_meta.get(slot as usize)
    };
    meta.map(|m| m.unit.clone()).unwrap_or_default()
}

const fn ir_metric(ty: IrType) -> MetricType {
    match ty {
        IrType::Bool => MetricType::Bool,
        IrType::Int => MetricType::Int,
        IrType::Dint => MetricType::Dint,
        IrType::Real => MetricType::Real,
        IrType::Time => MetricType::Time,
        IrType::Lint => MetricType::Lint,
    }
}
