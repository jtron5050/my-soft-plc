//! Schema-hash helper over [`plc_ir::RetainLayout`].

use plc_ir::RetainLayout;

use crate::crc::crc32;

/// CRC-32 of name-sorted records: `name \\0 type_tag offset_le retain_size_le`.
///
/// Labels the NV file; remap identity is still `(name, type)` only.
#[must_use]
pub fn schema_hash(layout: &RetainLayout) -> u32 {
    let mut buf = Vec::new();
    for sym in &layout.symbols {
        buf.extend_from_slice(sym.name.as_bytes());
        buf.push(0);
        buf.push(sym.ty as u8);
        buf.extend_from_slice(&sym.offset.to_le_bytes());
        buf.extend_from_slice(&layout.retain_size.to_le_bytes());
    }
    crc32(&buf)
}

#[cfg(test)]
mod tests {
    use plc_ir::{IrType, RetainLayout, RetainSymbol};

    use super::*;

    #[test]
    fn schema_hash_stable_and_order_independent() {
        let a = RetainLayout::new(
            8,
            vec![
                RetainSymbol::new("b", IrType::Int, 2),
                RetainSymbol::new("a", IrType::Bool, 0),
            ],
        )
        .unwrap();
        let b = RetainLayout::new(
            8,
            vec![
                RetainSymbol::new("a", IrType::Bool, 0),
                RetainSymbol::new("b", IrType::Int, 2),
            ],
        )
        .unwrap();
        assert_eq!(schema_hash(&a), schema_hash(&b));
        let c = RetainLayout::new(8, vec![RetainSymbol::new("a", IrType::Bool, 0)]).unwrap();
        assert_ne!(schema_hash(&a), schema_hash(&c));
    }
}
