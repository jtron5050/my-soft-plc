//! Per-tag quality plane (architecture: process image quality side plane).

/// Tag / module quality as stored in the quality plane (`u8`).
///
/// Normative encoding from the architecture design:
/// - `Good = 0`
/// - `Uncertain = 1`
/// - `Bad = 2`
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Quality {
    /// Value is trusted for control use.
    #[default]
    Good = 0,
    /// Non-fatal degraded sensor / timestamp uncertainty (e.g. unsynced NTP).
    Uncertain = 1,
    /// Communication loss, driver fault, or stale beyond `stale_ms`.
    Bad = 2,
}

impl Quality {
    /// Convert from the wire / image `u8` encoding.
    ///
    /// Unknown values map to [`Quality::Bad`] (fail closed).
    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Good,
            1 => Self::Uncertain,
            _ => Self::Bad,
        }
    }

    /// Wire / image encoding.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// `true` when logic may treat the value as control-ready.
    #[must_use]
    pub const fn is_good(self) -> bool {
        matches!(self, Self::Good)
    }

    /// `true` when the mapper should apply bad-quality output policy.
    #[must_use]
    pub const fn is_bad(self) -> bool {
        matches!(self, Self::Bad)
    }
}

impl From<Quality> for u8 {
    fn from(value: Quality) -> Self {
        value.as_u8()
    }
}

impl From<u8> for Quality {
    fn from(value: u8) -> Self {
        Self::from_u8(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_matches_architecture() {
        assert_eq!(Quality::Good.as_u8(), 0);
        assert_eq!(Quality::Uncertain.as_u8(), 1);
        assert_eq!(Quality::Bad.as_u8(), 2);
    }

    #[test]
    fn unknown_u8_is_bad() {
        assert_eq!(Quality::from_u8(3), Quality::Bad);
        assert_eq!(Quality::from_u8(255), Quality::Bad);
    }

    #[test]
    fn round_trip() {
        for q in [Quality::Good, Quality::Uncertain, Quality::Bad] {
            assert_eq!(Quality::from_u8(q.as_u8()), q);
        }
    }
}
