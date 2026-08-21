//! Operator mode requests and legal transitions (KD-17).

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;

use plc_types::OperatingMode;

use crate::error::ScanError;

/// Pending operator request (written by [`ScanHandle`], observed at boundaries).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeRequest {
    /// Enter STOP.
    Stop,
    /// Enter RUN (from STOP or SIM).
    Run,
    /// Enter SIM (from STOP only).
    Sim,
    /// Leave FAULT into STOP (explicit RUN required after).
    FaultReset,
}

const REQ_NONE: u8 = 0;
const REQ_STOP: u8 = 1;
const REQ_RUN: u8 = 2;
const REQ_SIM: u8 = 3;
const REQ_FAULT_RESET: u8 = 4;

const MODE_STOP: u8 = 0;
const MODE_RUN: u8 = 1;
const MODE_FAULT: u8 = 2;
const MODE_SIM: u8 = 3;

/// Encode [`OperatingMode`] for atomics.
#[must_use]
pub const fn encode_mode(mode: OperatingMode) -> u8 {
    match mode {
        OperatingMode::Stop => MODE_STOP,
        OperatingMode::Run => MODE_RUN,
        OperatingMode::Fault => MODE_FAULT,
        OperatingMode::Sim => MODE_SIM,
    }
}

/// Decode an atomic mode value (unknown → STOP).
#[must_use]
pub const fn decode_mode(raw: u8) -> OperatingMode {
    match raw {
        MODE_RUN => OperatingMode::Run,
        MODE_FAULT => OperatingMode::Fault,
        MODE_SIM => OperatingMode::Sim,
        _ => OperatingMode::Stop,
    }
}

fn encode_request(req: ModeRequest) -> u8 {
    match req {
        ModeRequest::Stop => REQ_STOP,
        ModeRequest::Run => REQ_RUN,
        ModeRequest::Sim => REQ_SIM,
        ModeRequest::FaultReset => REQ_FAULT_RESET,
    }
}

fn decode_request(raw: u8) -> Option<ModeRequest> {
    match raw {
        REQ_STOP => Some(ModeRequest::Stop),
        REQ_RUN => Some(ModeRequest::Run),
        REQ_SIM => Some(ModeRequest::Sim),
        REQ_FAULT_RESET => Some(ModeRequest::FaultReset),
        _ => None,
    }
}

/// Shared mode cell + pending request (non-RT writers, RT reader).
#[derive(Debug, Clone)]
pub struct ModeCell {
    mode: Arc<AtomicU8>,
    pending: Arc<AtomicU8>,
    rejected: Arc<AtomicU64>,
}

impl ModeCell {
    /// Start in STOP with no pending request.
    #[must_use]
    pub fn new() -> Self {
        Self {
            mode: Arc::new(AtomicU8::new(MODE_STOP)),
            pending: Arc::new(AtomicU8::new(REQ_NONE)),
            rejected: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Current mode.
    #[must_use]
    pub fn mode(&self) -> OperatingMode {
        decode_mode(self.mode.load(Ordering::Acquire))
    }

    /// Store mode (engine only — FAULT path).
    pub fn set_mode(&self, mode: OperatingMode) {
        self.mode.store(encode_mode(mode), Ordering::Release);
    }

    /// Queue a request (handle / tests). Last writer wins.
    pub fn request(&self, req: ModeRequest) {
        self.pending.store(encode_request(req), Ordering::Release);
    }

    /// Rejected-request counter.
    #[must_use]
    pub fn rejected(&self) -> u64 {
        self.rejected.load(Ordering::Relaxed)
    }

    /// Drop a pending request and count it as rejected (phase `swapping`).
    pub fn reject_pending(&self) {
        let raw = self.pending.swap(REQ_NONE, Ordering::AcqRel);
        if decode_request(raw).is_some() {
            self.rejected.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Apply a pending request at an invocation boundary.
    ///
    /// Returns the new mode and whether forces should be cleared.
    pub fn apply_pending(&self) -> Result<(OperatingMode, bool), ScanError> {
        let raw = self.pending.swap(REQ_NONE, Ordering::AcqRel);
        let Some(req) = decode_request(raw) else {
            return Ok((self.mode(), false));
        };
        match apply_request(self.mode(), req) {
            Ok(next) => {
                let clear_forces = matches!(req, ModeRequest::Stop | ModeRequest::FaultReset)
                    || next == OperatingMode::Stop;
                self.set_mode(next);
                Ok((next, clear_forces || next == OperatingMode::Fault))
            }
            Err(e) => {
                self.rejected.fetch_add(1, Ordering::Relaxed);
                Err(e)
            }
        }
    }
}

impl Default for ModeCell {
    fn default() -> Self {
        Self::new()
    }
}

/// Pure transition table.
pub fn apply_request(current: OperatingMode, req: ModeRequest) -> Result<OperatingMode, ScanError> {
    match (current, req) {
        (OperatingMode::Fault, ModeRequest::Stop) => Err(ScanError::invalid_state(
            "STOP while FAULT (FAULT_RESET first)",
        )),
        (_, ModeRequest::Stop) => Ok(OperatingMode::Stop),
        (OperatingMode::Stop | OperatingMode::Sim, ModeRequest::Run) => Ok(OperatingMode::Run),
        (OperatingMode::Run, ModeRequest::Run) => Ok(OperatingMode::Run),
        (OperatingMode::Stop, ModeRequest::Sim) => Ok(OperatingMode::Sim),
        (OperatingMode::Sim, ModeRequest::Sim) => Ok(OperatingMode::Sim),
        (OperatingMode::Fault, ModeRequest::FaultReset) => Ok(OperatingMode::Stop),
        (OperatingMode::Fault, ModeRequest::Run) => Err(ScanError::invalid_state(
            "RUN while FAULT (FAULT_RESET first)",
        )),
        (OperatingMode::Fault, ModeRequest::Sim) => {
            Err(ScanError::invalid_state("SIM while FAULT"))
        }
        (OperatingMode::Run, ModeRequest::Sim) => Err(ScanError::invalid_state("SIM from RUN")),
        (_, ModeRequest::FaultReset) => {
            Err(ScanError::invalid_state("FAULT_RESET while not FAULT"))
        }
    }
}

/// Non-RT handle for mode requests.
#[derive(Debug, Clone)]
pub struct ScanHandle {
    cell: ModeCell,
}

impl ScanHandle {
    pub(crate) fn new(cell: ModeCell) -> Self {
        Self { cell }
    }

    /// Queue a mode request (observed at the next invocation boundary).
    pub fn request_mode(&self, req: ModeRequest) {
        self.cell.request(req);
    }

    /// Last observed mode.
    #[must_use]
    pub fn mode(&self) -> OperatingMode {
        self.cell.mode()
    }

    /// How many requests were rejected.
    #[must_use]
    pub fn mode_rejected(&self) -> u64 {
        self.cell.rejected()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sim_only_from_stop() {
        assert!(apply_request(OperatingMode::Run, ModeRequest::Sim).is_err());
        assert_eq!(
            apply_request(OperatingMode::Stop, ModeRequest::Sim).unwrap(),
            OperatingMode::Sim
        );
    }

    #[test]
    fn fault_reset_to_stop() {
        assert_eq!(
            apply_request(OperatingMode::Fault, ModeRequest::FaultReset).unwrap(),
            OperatingMode::Stop
        );
        assert!(apply_request(OperatingMode::Fault, ModeRequest::Run).is_err());
    }
}
