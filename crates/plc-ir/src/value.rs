//! IR value type tags (Appendix A.1).

/// Abstract-machine type tag (`u4` in design; stored as u8).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IrType {
    /// BOOL.
    Bool = 0,
    /// INT i16.
    Int = 1,
    /// DINT i32.
    Dint = 2,
    /// REAL f32.
    Real = 3,
    /// TIME i32 ms.
    Time = 4,
    /// LINT i64 (optional ops).
    Lint = 5,
}

impl IrType {
    /// Parse from tag nibble / byte.
    #[must_use]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Bool),
            1 => Some(Self::Int),
            2 => Some(Self::Dint),
            3 => Some(Self::Real),
            4 => Some(Self::Time),
            5 => Some(Self::Lint),
            _ => None,
        }
    }

    /// Whether this is a numeric type for ADD/SUB/etc.
    #[must_use]
    pub const fn is_numeric(self) -> bool {
        matches!(
            self,
            Self::Int | Self::Dint | Self::Real | Self::Time | Self::Lint
        )
    }

    /// Whether bitwise ops apply.
    #[must_use]
    pub const fn is_integral(self) -> bool {
        matches!(
            self,
            Self::Bool | Self::Int | Self::Dint | Self::Time | Self::Lint
        )
    }
}
