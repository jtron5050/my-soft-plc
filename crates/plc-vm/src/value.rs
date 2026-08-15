//! Tagged runtime values for the abstract machine stack and image slots.

use plc_fb_primitives::StackValue;
use plc_ir::IrType;

/// 16-byte-class tagged value (implementation may pack; abstract machine uses tags).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VmValue {
    /// BOOL.
    Bool(bool),
    /// INT (i16).
    Int(i16),
    /// DINT (i32).
    Dint(i32),
    /// REAL (f32).
    Real(f32),
    /// TIME (i32 ms).
    Time(i32),
    /// LINT (i64).
    Lint(i64),
}

impl VmValue {
    /// Type tag.
    #[must_use]
    pub const fn ir_type(self) -> IrType {
        match self {
            Self::Bool(_) => IrType::Bool,
            Self::Int(_) => IrType::Int,
            Self::Dint(_) => IrType::Dint,
            Self::Real(_) => IrType::Real,
            Self::Time(_) => IrType::Time,
            Self::Lint(_) => IrType::Lint,
        }
    }

    /// Zero of the given type.
    #[must_use]
    pub const fn zero(ty: IrType) -> Self {
        match ty {
            IrType::Bool => Self::Bool(false),
            IrType::Int => Self::Int(0),
            IrType::Dint => Self::Dint(0),
            IrType::Real => Self::Real(0.0),
            IrType::Time => Self::Time(0),
            IrType::Lint => Self::Lint(0),
        }
    }

    /// Byte width when stored in the data/retain byte image.
    #[must_use]
    pub const fn byte_width(self) -> usize {
        match self {
            Self::Bool(_) => 1,
            Self::Int(_) => 2,
            Self::Dint(_) | Self::Real(_) | Self::Time(_) => 4,
            Self::Lint(_) => 8,
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

    /// Numeric as i64 when integral.
    #[must_use]
    pub fn as_i64(self) -> Option<i64> {
        match self {
            Self::Bool(b) => Some(i64::from(b)),
            Self::Int(v) => Some(i64::from(v)),
            Self::Dint(v) | Self::Time(v) => Some(i64::from(v)),
            Self::Lint(v) => Some(v),
            Self::Real(_) => None,
        }
    }

    /// Convert to primitive dispatcher scalar.
    #[must_use]
    pub fn to_stack_value(self) -> StackValue {
        match self {
            Self::Bool(b) => StackValue::Bool(b),
            Self::Int(v) => StackValue::Dint(i32::from(v)),
            Self::Dint(v) | Self::Time(v) => StackValue::Dint(v),
            Self::Real(v) => StackValue::Real(v),
            Self::Lint(v) => StackValue::Dint(v as i32),
        }
    }

    /// Convert from primitive dispatcher scalar with preferred type hint.
    #[must_use]
    pub fn from_stack_value(v: StackValue, prefer_time: bool) -> Self {
        match v {
            StackValue::Bool(b) => Self::Bool(b),
            StackValue::Dint(d) if prefer_time => Self::Time(d),
            StackValue::Dint(d) => Self::Dint(d),
            StackValue::Real(r) => Self::Real(r),
        }
    }

    /// Convert type (CONV opcode).
    #[must_use]
    pub fn convert(self, target: IrType) -> Self {
        match target {
            IrType::Bool => Self::Bool(match self {
                Self::Bool(b) => b,
                Self::Int(v) => v != 0,
                Self::Dint(v) | Self::Time(v) => v != 0,
                Self::Real(v) => v != 0.0,
                Self::Lint(v) => v != 0,
            }),
            IrType::Int => Self::Int(match self {
                Self::Bool(b) => i16::from(b),
                Self::Int(v) => v,
                Self::Dint(v) | Self::Time(v) => v as i16,
                Self::Real(v) => v as i16,
                Self::Lint(v) => v as i16,
            }),
            IrType::Dint => Self::Dint(match self {
                Self::Bool(b) => i32::from(b),
                Self::Int(v) => i32::from(v),
                Self::Dint(v) | Self::Time(v) => v,
                Self::Real(v) => v as i32,
                Self::Lint(v) => v as i32,
            }),
            IrType::Real => Self::Real(match self {
                Self::Bool(b) => {
                    if b {
                        1.0
                    } else {
                        0.0
                    }
                }
                Self::Int(v) => f32::from(v),
                Self::Dint(v) | Self::Time(v) => v as f32,
                Self::Real(v) => v,
                Self::Lint(v) => v as f32,
            }),
            IrType::Time => Self::Time(match self {
                Self::Bool(b) => i32::from(b),
                Self::Int(v) => i32::from(v),
                Self::Dint(v) | Self::Time(v) => v,
                Self::Real(v) => v as i32,
                Self::Lint(v) => v as i32,
            }),
            IrType::Lint => Self::Lint(match self {
                Self::Bool(b) => i64::from(b),
                Self::Int(v) => i64::from(v),
                Self::Dint(v) | Self::Time(v) => i64::from(v),
                Self::Real(v) => v as i64,
                Self::Lint(v) => v,
            }),
        }
    }
}

impl Default for VmValue {
    fn default() -> Self {
        Self::Bool(false)
    }
}
