//! Process-image region identifiers (`%I` / `%Q` / `%M` / retain).

/// Logical process-image region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageRegion {
    /// Inputs (`%I`) — snapshot at task start.
    Input,
    /// Outputs (`%Q`) — written at end of owning task; safe on FAULT.
    Output,
    /// Volatile memory (`%M`) — working state; cold on activate.
    Memory,
    /// Retained memory (`%R` / retain map) — non-volatile symbolic map.
    Retain,
}

impl ImageRegion {
    /// IEC-style percent prefix used in diagnostics and docs.
    #[must_use]
    pub const fn percent_prefix(self) -> &'static str {
        match self {
            Self::Input => "%I",
            Self::Output => "%Q",
            Self::Memory => "%M",
            Self::Retain => "%R",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes() {
        assert_eq!(ImageRegion::Input.percent_prefix(), "%I");
        assert_eq!(ImageRegion::Output.percent_prefix(), "%Q");
        assert_eq!(ImageRegion::Memory.percent_prefix(), "%M");
        assert_eq!(ImageRegion::Retain.percent_prefix(), "%R");
    }
}
