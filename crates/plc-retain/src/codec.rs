//! Symbolic retain payload codec (no I/O).

use plc_ir::{IrType, RetainLayout};

use crate::error::RetainError;

/// One decoded retain record (name + type + raw value bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainRecord {
    /// IEC path.
    pub name: String,
    /// Stored type.
    pub ty: IrType,
    /// Little-endian value bytes (`ty.byte_width()` long).
    pub value: Vec<u8>,
}

impl RetainRecord {
    /// Construct a record.
    #[must_use]
    pub fn new(name: impl Into<String>, ty: IrType, value: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            ty,
            value,
        }
    }
}

/// Encode layout symbols from a retain image into the on-disk payload.
///
/// Records are written in name-sorted layout order.
pub fn encode_records(layout: &RetainLayout, image: &[u8]) -> Result<Vec<u8>, RetainError> {
    if image.len() != layout.retain_size as usize {
        return Err(RetainError::ImageSize {
            expected: layout.retain_size,
            actual: image.len(),
        });
    }
    let mut out = Vec::new();
    for sym in &layout.symbols {
        let start = sym.offset as usize;
        let width = sym.ty.byte_width();
        let name = sym.name.as_bytes();
        let name_len = u16::try_from(name.len()).map_err(|_| {
            RetainError::codec(format!("symbol name {} exceeds u16 length", sym.name))
        })?;
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(name);
        out.push(sym.ty as u8);
        out.push(0);
        out.extend_from_slice(&image[start..start + width]);
    }
    Ok(out)
}

/// Decode a symbolic payload into records.
pub fn decode_records(payload: &[u8]) -> Result<Vec<RetainRecord>, RetainError> {
    let mut recs = Vec::new();
    let mut i = 0;
    while i < payload.len() {
        if i + 2 > payload.len() {
            return Err(RetainError::codec("truncated name_len"));
        }
        let name_len = u16::from_le_bytes([payload[i], payload[i + 1]]) as usize;
        i += 2;
        if i + name_len + 2 > payload.len() {
            return Err(RetainError::codec("truncated name or type"));
        }
        let name = std::str::from_utf8(&payload[i..i + name_len])
            .map_err(|_| RetainError::codec("symbol name is not UTF-8"))?
            .to_string();
        i += name_len;
        let tag = payload[i];
        let pad = payload[i + 1];
        i += 2;
        if pad != 0 {
            return Err(RetainError::codec("nonzero record pad"));
        }
        let ty = IrType::from_u8(tag)
            .ok_or_else(|| RetainError::codec(format!("unknown type tag {tag}")))?;
        let width = ty.byte_width();
        if i + width > payload.len() {
            return Err(RetainError::codec(format!(
                "truncated value for {name} ({width} bytes)"
            )));
        }
        let value = payload[i..i + width].to_vec();
        i += width;
        recs.push(RetainRecord { name, ty, value });
    }
    Ok(recs)
}

/// Extract records from a live image using `layout` (no encode step).
pub fn records_from_image(
    layout: &RetainLayout,
    image: &[u8],
) -> Result<Vec<RetainRecord>, RetainError> {
    if image.len() != layout.retain_size as usize {
        return Err(RetainError::ImageSize {
            expected: layout.retain_size,
            actual: image.len(),
        });
    }
    Ok(layout
        .symbols
        .iter()
        .map(|sym| {
            let start = sym.offset as usize;
            let width = sym.ty.byte_width();
            RetainRecord {
                name: sym.name.clone(),
                ty: sym.ty,
                value: image[start..start + width].to_vec(),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use plc_ir::{IrType, RetainLayout, RetainSymbol};

    use super::*;

    fn sample_layout() -> RetainLayout {
        RetainLayout::new(
            16,
            vec![
                RetainSymbol::new("flag", IrType::Bool, 0),
                RetainSymbol::new("hours", IrType::Dint, 4),
                RetainSymbol::new("gain", IrType::Real, 8),
                RetainSymbol::new("pt", IrType::Time, 12),
            ],
        )
        .unwrap()
    }

    #[test]
    fn codec_roundtrip() {
        let layout = sample_layout();
        let mut image = vec![0u8; 16];
        image[0] = 1;
        image[4..8].copy_from_slice(&42i32.to_le_bytes());
        let bits = 1.5f32.to_bits().to_le_bytes();
        image[8..12].copy_from_slice(&bits);
        image[12..16].copy_from_slice(&1000i32.to_le_bytes());

        let payload = encode_records(&layout, &image).unwrap();
        let recs = decode_records(&payload).unwrap();
        assert_eq!(recs.len(), 4);
        assert_eq!(recs[0].name, "flag");
        assert_eq!(recs[0].ty, IrType::Bool);
        assert_eq!(recs[0].value, vec![1]);
        assert_eq!(recs[1].name, "gain");
        assert_eq!(recs[1].ty, IrType::Real);
        assert_eq!(recs[1].value, bits);
        assert_eq!(recs[2].name, "hours");
        assert_eq!(recs[3].name, "pt");
        assert_eq!(recs[3].value, 1000i32.to_le_bytes());

        let again = encode_records(&layout, &image).unwrap();
        assert_eq!(payload, again);
    }

    #[test]
    fn decode_rejects_truncated() {
        assert!(decode_records(&[0x01]).is_err());
    }
}
