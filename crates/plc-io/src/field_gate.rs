//! Shared operating-mode mirror for non-RT field workers.
//!
//! The scan thread stores the mode at the start of each input and output phase.
//! A Modbus worker reads it to suppress field writes in SIM without taking the
//! scan lock.

use std::sync::atomic::{AtomicU8, Ordering};

use plc_types::OperatingMode;

/// Atomic copy of [`OperatingMode`] for drivers that must not block the scan.
#[derive(Debug)]
pub struct FieldGate {
    mode: AtomicU8,
}

impl Default for FieldGate {
    fn default() -> Self {
        Self::new()
    }
}

impl FieldGate {
    /// Start in STOP so the first field writes are safe-state until RUN.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            mode: AtomicU8::new(MODE_STOP),
        }
    }

    /// Publish the scan's current mode.
    pub fn set_mode(&self, mode: OperatingMode) {
        self.mode.store(mode_code(mode), Ordering::Release);
    }

    /// Latest mode stored by the scan (STOP if none yet).
    #[must_use]
    pub fn mode(&self) -> OperatingMode {
        mode_from(self.mode.load(Ordering::Acquire))
    }
}

const MODE_STOP: u8 = 0;
const MODE_RUN: u8 = 1;
const MODE_FAULT: u8 = 2;
const MODE_SIM: u8 = 3;

const fn mode_code(mode: OperatingMode) -> u8 {
    match mode {
        OperatingMode::Stop => MODE_STOP,
        OperatingMode::Run => MODE_RUN,
        OperatingMode::Fault => MODE_FAULT,
        OperatingMode::Sim => MODE_SIM,
    }
}

fn mode_from(code: u8) -> OperatingMode {
    match code {
        MODE_RUN => OperatingMode::Run,
        MODE_FAULT => OperatingMode::Fault,
        MODE_SIM => OperatingMode::Sim,
        _ => OperatingMode::Stop,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_stop_and_round_trips() {
        let gate = FieldGate::new();
        assert_eq!(gate.mode(), OperatingMode::Stop);
        gate.set_mode(OperatingMode::Sim);
        assert_eq!(gate.mode(), OperatingMode::Sim);
        gate.set_mode(OperatingMode::Run);
        assert_eq!(gate.mode(), OperatingMode::Run);
        gate.set_mode(OperatingMode::Fault);
        assert_eq!(gate.mode(), OperatingMode::Fault);
    }
}
