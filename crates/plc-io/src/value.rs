//! Runtime PLC scalar values used in the process image.

use crate::map::ValueType;

/// Tagged process value (mirrors IR scalar types used at the I/O boundary).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlcValue {
    /// Boolean.
    Bool(bool),
    /// 16-bit signed.
    Int(i16),
    /// 32-bit signed.
    Dint(i32),
    /// IEEE-754 binary32.
    Real(f32),
    /// Duration / TIME in milliseconds.
    Time(i32),
}

impl PlcValue {
    /// Zero / false default for a type.
    #[must_use]
    pub const fn default_of(ty: ValueType) -> Self {
        match ty {
            ValueType::Bool => Self::Bool(false),
            ValueType::Int => Self::Int(0),
            ValueType::Dint | ValueType::Time => Self::Dint(0),
            ValueType::Real => Self::Real(0.0),
        }
    }

    /// Type tag of this value.
    #[must_use]
    pub const fn value_type(self) -> ValueType {
        match self {
            Self::Bool(_) => ValueType::Bool,
            Self::Int(_) => ValueType::Int,
            Self::Dint(_) => ValueType::Dint,
            Self::Real(_) => ValueType::Real,
            Self::Time(_) => ValueType::Time,
        }
    }

    /// Interpret as BOOL (non-bool → false).
    #[must_use]
    pub const fn as_bool(self) -> bool {
        match self {
            Self::Bool(b) => b,
            _ => false,
        }
    }

    /// Interpret as f32 when possible.
    #[must_use]
    pub fn as_f32(self) -> Option<f32> {
        match self {
            Self::Real(v) => Some(v),
            Self::Int(v) => Some(f32::from(v)),
            Self::Dint(v) | Self::Time(v) => Some(v as f32),
            Self::Bool(b) => Some(if b { 1.0 } else { 0.0 }),
        }
    }
}
