//! Operator-visible control mode and program package phase (KD-17).

/// Mutually exclusive operator-visible control modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum OperatingMode {
    /// Logic not executed; outputs follow `stop_output_policy` (default safe).
    #[default]
    Stop,
    /// Cyclic cooperative execution.
    Run,
    /// Safe outputs held; requires `FAULT_RESET` then explicit `Run`.
    Fault,
    /// Logic runs against the simulation driver only (no field writes).
    Sim,
}

impl OperatingMode {
    /// Stable lowercase wire / REST name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stop => "STOP",
            Self::Run => "RUN",
            Self::Fault => "FAULT",
            Self::Sim => "SIM",
        }
    }

    /// Parse a REST / config mode token (case-insensitive ASCII).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            s if s.eq_ignore_ascii_case("STOP") => Some(Self::Stop),
            s if s.eq_ignore_ascii_case("RUN") => Some(Self::Run),
            s if s.eq_ignore_ascii_case("FAULT") => Some(Self::Fault),
            s if s.eq_ignore_ascii_case("SIM") => Some(Self::Sim),
            _ => None,
        }
    }

    /// Whether cyclic logic executes in this mode.
    #[must_use]
    pub const fn executes_logic(self) -> bool {
        matches!(self, Self::Run | Self::Sim)
    }
}

/// Program package lifecycle phase (orthogonal to [`OperatingMode`]).
///
/// There is no operator mode named `LOAD`; validation/arming can occur while
/// `mode` is `Run` or `Stop`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ProgramPhase {
    /// No pending package operation.
    #[default]
    Idle,
    /// Upload accepted; signature / IR / retain checks running (non-RT).
    Validating,
    /// Buffer B ready; waiting for activate.
    Armed,
    /// Epoch critical section in progress or scheduled.
    Swapping,
}

impl ProgramPhase {
    /// Stable lowercase wire / REST name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Validating => "validating",
            Self::Armed => "armed",
            Self::Swapping => "swapping",
        }
    }

    /// Parse a REST / status phase token (case-insensitive ASCII).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            s if s.eq_ignore_ascii_case("idle") => Some(Self::Idle),
            s if s.eq_ignore_ascii_case("validating") => Some(Self::Validating),
            s if s.eq_ignore_ascii_case("armed") => Some(Self::Armed),
            s if s.eq_ignore_ascii_case("swapping") => Some(Self::Swapping),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_wire_names() {
        assert_eq!(OperatingMode::Stop.as_str(), "STOP");
        assert_eq!(OperatingMode::parse("run"), Some(OperatingMode::Run));
        assert_eq!(OperatingMode::parse("nope"), None);
    }

    #[test]
    fn phase_wire_names() {
        assert_eq!(ProgramPhase::Armed.as_str(), "armed");
        assert_eq!(
            ProgramPhase::parse("SWAPPING"),
            Some(ProgramPhase::Swapping)
        );
    }

    #[test]
    fn executes_logic_only_in_run_or_sim() {
        assert!(!OperatingMode::Stop.executes_logic());
        assert!(OperatingMode::Run.executes_logic());
        assert!(!OperatingMode::Fault.executes_logic());
        assert!(OperatingMode::Sim.executes_logic());
    }
}
