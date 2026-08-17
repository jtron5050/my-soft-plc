//! Symbolic retain layout (name + type + offset) shared with the NV store
//! and (later) the package manifest.

use crate::error::IrError;
use crate::value::IrType;

/// One retained symbol: IEC path, type, and byte offset in the retain segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainSymbol {
    /// IEC path (e.g. `Line.Hours`).
    pub name: String,
    /// Value type.
    pub ty: IrType,
    /// Byte offset in the retain segment.
    pub offset: u32,
}

impl RetainSymbol {
    /// Construct a symbol.
    #[must_use]
    pub fn new(name: impl Into<String>, ty: IrType, offset: u32) -> Self {
        Self {
            name: name.into(),
            ty,
            offset,
        }
    }

    /// Byte width of the stored value.
    #[must_use]
    pub const fn byte_width(&self) -> usize {
        self.ty.byte_width()
    }

    /// Exclusive end offset.
    #[must_use]
    pub fn end_offset(&self) -> u32 {
        self.offset.saturating_add(self.ty.byte_width() as u32)
    }
}

/// Validated retain segment layout: unique names, in-bounds, non-overlapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainLayout {
    /// Retain segment size in bytes.
    pub retain_size: u32,
    /// Symbols sorted by name.
    pub symbols: Vec<RetainSymbol>,
}

impl RetainLayout {
    /// Sort by name and reject duplicates, out-of-bounds, or overlapping ranges.
    pub fn new(retain_size: u32, mut symbols: Vec<RetainSymbol>) -> Result<Self, IrError> {
        symbols.sort_by(|a, b| a.name.cmp(&b.name));
        for window in symbols.windows(2) {
            if window[0].name == window[1].name {
                return Err(IrError::RetainLayout(format!(
                    "duplicate symbol {}",
                    window[0].name
                )));
            }
        }
        for sym in &symbols {
            if sym.name.is_empty() {
                return Err(IrError::RetainLayout("empty symbol name".into()));
            }
            let end = u64::from(sym.offset) + sym.ty.byte_width() as u64;
            if end > u64::from(retain_size) {
                return Err(IrError::RetainLayout(format!(
                    "symbol {} at {}+{} exceeds retain_size {retain_size}",
                    sym.name,
                    sym.offset,
                    sym.ty.byte_width()
                )));
            }
        }
        // Overlap check on offset-sorted copy (names stay name-sorted in `symbols`).
        let mut by_off: Vec<&RetainSymbol> = symbols.iter().collect();
        by_off.sort_by_key(|s| s.offset);
        for window in by_off.windows(2) {
            if window[0].end_offset() > window[1].offset {
                return Err(IrError::RetainLayout(format!(
                    "symbols {} and {} overlap",
                    window[0].name, window[1].name
                )));
            }
        }
        Ok(Self {
            retain_size,
            symbols,
        })
    }

    /// Empty layout of the given size.
    #[must_use]
    pub fn empty(retain_size: u32) -> Self {
        Self {
            retain_size,
            symbols: Vec::new(),
        }
    }

    /// Look up a symbol by exact name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&RetainSymbol> {
        self.symbols
            .binary_search_by(|s| s.name.as_str().cmp(name))
            .ok()
            .map(|i| &self.symbols[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_sorts_and_looks_up() {
        let layout = RetainLayout::new(
            16,
            vec![
                RetainSymbol::new("b", IrType::Dint, 4),
                RetainSymbol::new("a", IrType::Bool, 0),
            ],
        )
        .unwrap();
        assert_eq!(layout.symbols[0].name, "a");
        assert_eq!(layout.symbols[1].name, "b");
        assert_eq!(layout.get("b").unwrap().offset, 4);
        assert!(layout.get("missing").is_none());
    }

    #[test]
    fn layout_rejects_overlap_and_oob() {
        let oob = RetainLayout::new(2, vec![RetainSymbol::new("x", IrType::Dint, 0)]);
        assert!(oob.is_err(), "DINT at 0 needs 4 bytes");

        let overlap = RetainLayout::new(
            16,
            vec![
                RetainSymbol::new("a", IrType::Dint, 0),
                RetainSymbol::new("b", IrType::Int, 2),
            ],
        );
        assert!(overlap.unwrap_err().to_string().contains("overlap"));

        let dup = RetainLayout::new(
            16,
            vec![
                RetainSymbol::new("a", IrType::Bool, 0),
                RetainSymbol::new("a", IrType::Int, 2),
            ],
        );
        assert!(dup.unwrap_err().to_string().contains("duplicate"));
    }
}
