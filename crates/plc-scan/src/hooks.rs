//! Epoch / FirstScan hooks consumed by PR-10.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;

use plc_types::ProgramPhase;

const PHASE_IDLE: u8 = 0;
const PHASE_VALIDATING: u8 = 1;
const PHASE_ARMED: u8 = 2;
const PHASE_SWAPPING: u8 = 3;

fn encode_phase(p: ProgramPhase) -> u8 {
    match p {
        ProgramPhase::Idle => PHASE_IDLE,
        ProgramPhase::Validating => PHASE_VALIDATING,
        ProgramPhase::Armed => PHASE_ARMED,
        ProgramPhase::Swapping => PHASE_SWAPPING,
    }
}

fn decode_phase(raw: u8) -> ProgramPhase {
    match raw {
        PHASE_VALIDATING => ProgramPhase::Validating,
        PHASE_ARMED => ProgramPhase::Armed,
        PHASE_SWAPPING => ProgramPhase::Swapping,
        _ => ProgramPhase::Idle,
    }
}

struct Inner {
    phase: AtomicU8,
    activate_requested: AtomicBool,
    first_scan: Box<[AtomicBool]>,
    in_invocation: AtomicBool,
}

/// Quiet-point / FirstScan / phase cell for program-epoch (KD-4a).
///
/// The scan engine waits for [`EpochHooks::is_quiet`] before starting the
/// activate critical section, and publishes per-task FirstScan bits.
#[derive(Clone)]
pub struct EpochHooks {
    inner: Arc<Inner>,
}

impl EpochHooks {
    /// All FirstScan bits false, phase Idle.
    #[must_use]
    pub fn new(n_tasks: usize) -> Self {
        Self {
            inner: Arc::new(Inner {
                phase: AtomicU8::new(PHASE_IDLE),
                activate_requested: AtomicBool::new(false),
                first_scan: (0..n_tasks).map(|_| AtomicBool::new(false)).collect(),
                in_invocation: AtomicBool::new(false),
            }),
        }
    }

    /// Program phase (Idle until PR-10 writes otherwise).
    #[must_use]
    pub fn phase(&self) -> ProgramPhase {
        decode_phase(self.inner.phase.load(Ordering::Acquire))
    }

    /// Store phase (PR-10).
    pub fn set_phase(&self, phase: ProgramPhase) {
        self.inner
            .phase
            .store(encode_phase(phase), Ordering::Release);
    }

    /// Activate request flag.
    #[must_use]
    pub fn activate_requested(&self) -> bool {
        self.inner.activate_requested.load(Ordering::Acquire)
    }

    /// Set / clear activate request (PR-10).
    pub fn set_activate_requested(&self, v: bool) {
        self.inner.activate_requested.store(v, Ordering::Release);
    }

    /// True when no task invocation is in progress.
    #[must_use]
    pub fn is_quiet(&self) -> bool {
        !self.inner.in_invocation.load(Ordering::Acquire)
    }

    /// Scan path: mark invocation start/end.
    pub fn set_in_invocation(&self, v: bool) {
        self.inner.in_invocation.store(v, Ordering::Release);
    }

    /// Per-task FirstScan bit.
    #[must_use]
    pub fn first_scan(&self, task_idx: usize) -> bool {
        self.inner
            .first_scan
            .get(task_idx)
            .is_some_and(|b| b.load(Ordering::Acquire))
    }

    /// Set every task's FirstScan bit (activate / cold boot — PR-10).
    pub fn set_first_scan_all(&self, v: bool) {
        for b in &self.inner.first_scan {
            b.store(v, Ordering::Release);
        }
    }

    /// Clear FirstScan for one task after its first completed invocation.
    pub fn clear_first_scan(&self, task_idx: usize) {
        if let Some(b) = self.inner.first_scan.get(task_idx) {
            b.store(false, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_and_first_scan() {
        let h = EpochHooks::new(2);
        assert!(h.is_quiet());
        assert_eq!(h.phase(), ProgramPhase::Idle);
        h.set_in_invocation(true);
        assert!(!h.is_quiet());
        h.set_first_scan_all(true);
        assert!(h.first_scan(0));
        h.clear_first_scan(0);
        assert!(!h.first_scan(0));
        assert!(h.first_scan(1));
    }
}
