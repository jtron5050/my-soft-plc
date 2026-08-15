//! Byte-addressed data/retain segments and typed I/O image slots.

use plc_ir::IrType;

use crate::error::VmError;
use crate::value::VmValue;

/// Type tag byte: 0xFF = unset / unknown.
const TAG_UNSET: u8 = 0xFF;

/// Writable byte segment with per-offset start tags for typed load/store.
#[derive(Debug, Clone)]
pub struct ByteSegment {
    bytes: Vec<u8>,
    /// Tag at the starting offset of each stored value (`IrType` as u8 or `TAG_UNSET`).
    tags: Vec<u8>,
}

impl ByteSegment {
    /// Zero-filled segment of `size` bytes.
    #[must_use]
    pub fn zeros(size: usize) -> Self {
        Self {
            bytes: vec![0; size],
            tags: vec![TAG_UNSET; size],
        }
    }

    /// Length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// True when empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Raw byte slice (tests / diagnostics).
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Store a typed value at absolute byte offset.
    pub fn store(&mut self, offset: usize, value: VmValue, pc: usize) -> Result<(), VmError> {
        let width = value.byte_width();
        if offset.saturating_add(width) > self.bytes.len() {
            return Err(VmError::Bounds {
                pc,
                detail: format!(
                    "store offset {offset}+{width} exceeds segment {}",
                    self.bytes.len()
                ),
            });
        }
        match value {
            VmValue::Bool(b) => {
                self.bytes[offset] = u8::from(b);
            }
            VmValue::Int(v) => {
                self.bytes[offset..offset + 2].copy_from_slice(&v.to_le_bytes());
            }
            VmValue::Dint(v) | VmValue::Time(v) => {
                self.bytes[offset..offset + 4].copy_from_slice(&v.to_le_bytes());
            }
            VmValue::Real(v) => {
                self.bytes[offset..offset + 4].copy_from_slice(&v.to_bits().to_le_bytes());
            }
            VmValue::Lint(v) => {
                self.bytes[offset..offset + 8].copy_from_slice(&v.to_le_bytes());
            }
        }
        self.tags[offset] = value.ir_type() as u8;
        // Clear overlapping start tags in the payload range (except start).
        for t in self.tags.iter_mut().take(offset + width).skip(offset + 1) {
            *t = TAG_UNSET;
        }
        Ok(())
    }

    /// Load a typed value from absolute byte offset.
    ///
    /// Uses the tag written by the last store at this offset; defaults to BOOL
    /// when unset (matches cold BOOL-heavy instance layouts).
    pub fn load(&self, offset: usize, pc: usize) -> Result<VmValue, VmError> {
        if offset >= self.bytes.len() {
            return Err(VmError::Bounds {
                pc,
                detail: format!("load offset {offset} exceeds segment {}", self.bytes.len()),
            });
        }
        let ty = match self.tags.get(offset).copied().unwrap_or(TAG_UNSET) {
            TAG_UNSET => IrType::Bool,
            t => IrType::from_u8(t).unwrap_or(IrType::Bool),
        };
        self.load_as(offset, ty, pc)
    }

    fn load_as(&self, offset: usize, ty: IrType, pc: usize) -> Result<VmValue, VmError> {
        let width = VmValue::zero(ty).byte_width();
        if offset.saturating_add(width) > self.bytes.len() {
            return Err(VmError::Bounds {
                pc,
                detail: format!("load {ty:?} at {offset} OOB"),
            });
        }
        let v = match ty {
            IrType::Bool => VmValue::Bool(self.bytes[offset] != 0),
            IrType::Int => {
                let b: [u8; 2] = self.bytes[offset..offset + 2].try_into().unwrap();
                VmValue::Int(i16::from_le_bytes(b))
            }
            IrType::Dint => {
                let b: [u8; 4] = self.bytes[offset..offset + 4].try_into().unwrap();
                VmValue::Dint(i32::from_le_bytes(b))
            }
            IrType::Time => {
                let b: [u8; 4] = self.bytes[offset..offset + 4].try_into().unwrap();
                VmValue::Time(i32::from_le_bytes(b))
            }
            IrType::Real => {
                let b: [u8; 4] = self.bytes[offset..offset + 4].try_into().unwrap();
                VmValue::Real(f32::from_bits(u32::from_le_bytes(b)))
            }
            IrType::Lint => {
                let b: [u8; 8] = self.bytes[offset..offset + 8].try_into().unwrap();
                VmValue::Lint(i64::from_le_bytes(b))
            }
        };
        Ok(v)
    }

    /// Read BOOL at offset without requiring a prior store tag.
    #[must_use]
    pub fn load_bool_raw(&self, offset: usize) -> bool {
        self.bytes.get(offset).is_some_and(|b| *b != 0)
    }
}

/// Typed process image slots for `%I` / `%Q`.
#[derive(Debug, Clone)]
pub struct SlotImage {
    values: Vec<VmValue>,
    /// Per-input quality Good? (parallel to inputs only; outputs unused).
    quality_good: Vec<bool>,
}

impl SlotImage {
    /// Create `n` slots defaulting to BOOL false / Good.
    #[must_use]
    pub fn bools(n: usize) -> Self {
        Self {
            values: vec![VmValue::Bool(false); n],
            quality_good: vec![true; n],
        }
    }

    /// Slot count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Empty check.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Get value.
    pub fn get(&self, idx: usize, pc: usize) -> Result<VmValue, VmError> {
        self.values
            .get(idx)
            .copied()
            .ok_or_else(|| VmError::Bounds {
                pc,
                detail: format!("slot {idx}"),
            })
    }

    /// Set value.
    pub fn set(&mut self, idx: usize, value: VmValue, pc: usize) -> Result<(), VmError> {
        let slot = self.values.get_mut(idx).ok_or_else(|| VmError::Bounds {
            pc,
            detail: format!("slot {idx}"),
        })?;
        *slot = value;
        Ok(())
    }

    /// Quality Good? for input slot.
    pub fn quality_good(&self, idx: usize, pc: usize) -> Result<bool, VmError> {
        self.quality_good
            .get(idx)
            .copied()
            .ok_or_else(|| VmError::Bounds {
                pc,
                detail: format!("quality slot {idx}"),
            })
    }

    /// Set quality (mapper / tests).
    pub fn set_quality_good(&mut self, idx: usize, good: bool, pc: usize) -> Result<(), VmError> {
        let q = self
            .quality_good
            .get_mut(idx)
            .ok_or_else(|| VmError::Bounds {
                pc,
                detail: format!("quality slot {idx}"),
            })?;
        *q = good;
        Ok(())
    }

    /// Direct slice access for tests.
    #[must_use]
    pub fn values(&self) -> &[VmValue] {
        &self.values
    }
}
