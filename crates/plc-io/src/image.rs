//! Process image buffers: inputs, outputs, memory, and quality plane.

use plc_types::Quality;

use crate::error::IoError;
use crate::map::ValueType;
use crate::value::PlcValue;

/// Metadata for one typed image slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotMeta {
    /// Logical tag name (empty if anonymous).
    pub tag: String,
    /// Value type.
    pub ty: ValueType,
}

/// Typed slot storage cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TypedSlot {
    /// Current value.
    pub value: PlcValue,
    /// Quality for this slot (`Good` for `%M` by default).
    pub quality: Quality,
    /// True after a successful program or mapper write this arm.
    pub written: bool,
}

impl TypedSlot {
    /// Default zeroed slot of the given type.
    #[must_use]
    pub const fn zero(ty: ValueType) -> Self {
        Self {
            value: PlcValue::default_of(ty),
            quality: Quality::Good,
            written: false,
        }
    }
}

/// Full process image held by the scan engine.
#[derive(Debug, Clone)]
pub struct ProcessImage {
    /// `%I` slots.
    pub inputs: Vec<TypedSlot>,
    /// `%Q` slots.
    pub outputs: Vec<TypedSlot>,
    /// `%M` slots.
    pub memory: Vec<TypedSlot>,
    /// Input metadata (parallel to `inputs`).
    pub input_meta: Vec<SlotMeta>,
    /// Output metadata (parallel to `outputs`).
    pub output_meta: Vec<SlotMeta>,
    /// Memory metadata.
    pub memory_meta: Vec<SlotMeta>,
    /// Configured safe-state for each output (defaults to type zero).
    pub output_safe: Vec<PlcValue>,
}

impl ProcessImage {
    /// Allocate image regions with default types (BOOL) for the given counts.
    #[must_use]
    pub fn with_sizes(n_i: usize, n_q: usize, n_m: usize) -> Self {
        Self {
            inputs: vec![TypedSlot::zero(ValueType::Bool); n_i],
            outputs: vec![TypedSlot::zero(ValueType::Bool); n_q],
            memory: vec![TypedSlot::zero(ValueType::Bool); n_m],
            input_meta: (0..n_i)
                .map(|i| SlotMeta {
                    tag: format!("I{i}"),
                    ty: ValueType::Bool,
                })
                .collect(),
            output_meta: (0..n_q)
                .map(|i| SlotMeta {
                    tag: format!("Q{i}"),
                    ty: ValueType::Bool,
                })
                .collect(),
            memory_meta: (0..n_m)
                .map(|i| SlotMeta {
                    tag: format!("M{i}"),
                    ty: ValueType::Bool,
                })
                .collect(),
            output_safe: vec![PlcValue::Bool(false); n_q],
        }
    }

    /// Read an input slot.
    pub fn get_input(&self, idx: usize) -> Result<TypedSlot, IoError> {
        self.inputs
            .get(idx)
            .copied()
            .ok_or_else(|| IoError::Bounds(format!("input slot {idx}")))
    }

    /// Write an input slot (mapper path).
    pub fn set_input(
        &mut self,
        idx: usize,
        value: PlcValue,
        quality: Quality,
    ) -> Result<(), IoError> {
        let slot = self
            .inputs
            .get_mut(idx)
            .ok_or_else(|| IoError::Bounds(format!("input slot {idx}")))?;
        slot.value = value;
        slot.quality = quality;
        slot.written = true;
        Ok(())
    }

    /// Read an output slot.
    pub fn get_output(&self, idx: usize) -> Result<TypedSlot, IoError> {
        self.outputs
            .get(idx)
            .copied()
            .ok_or_else(|| IoError::Bounds(format!("output slot {idx}")))
    }

    /// Program write to `%Q`.
    pub fn set_output(&mut self, idx: usize, value: PlcValue) -> Result<(), IoError> {
        let slot = self
            .outputs
            .get_mut(idx)
            .ok_or_else(|| IoError::Bounds(format!("output slot {idx}")))?;
        slot.value = value;
        slot.written = true;
        Ok(())
    }

    /// Quality of input slot as BOOL Good?.
    #[must_use]
    pub fn input_quality_good(&self, idx: usize) -> bool {
        self.inputs.get(idx).is_some_and(|s| s.quality.is_good())
    }
}
