//! Software overrun watchdog and hardware-watchdog stroke trait.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Hardware watchdog stroke hook.
///
/// PR-07 ships [`NullWatchdog`] and [`RecordingWatchdog`] only. Opening
/// `/dev/watchdog` is deferred (deployment / PR-19).
pub trait HardwareWatchdog: Send {
    /// Stroke after a successful invocation completion.
    ///
    /// Called in STOP, RUN, SIM, and FAULT (prolonged FAULT must not reboot).
    fn stroke(&mut self);
}

/// No-op hardware watchdog (`watchdog.hardware_enabled: false`).
#[derive(Debug, Default)]
pub struct NullWatchdog;

impl HardwareWatchdog for NullWatchdog {
    fn stroke(&mut self) {}
}

/// Counts strokes for tests.
#[derive(Debug, Clone)]
pub struct RecordingWatchdog {
    /// Shared stroke counter.
    pub strokes: Arc<AtomicU64>,
}

impl RecordingWatchdog {
    /// New counter at zero.
    #[must_use]
    pub fn new() -> Self {
        Self {
            strokes: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Current stroke count.
    #[must_use]
    pub fn count(&self) -> u64 {
        self.strokes.load(Ordering::Relaxed)
    }
}

impl Default for RecordingWatchdog {
    fn default() -> Self {
        Self::new()
    }
}

impl HardwareWatchdog for RecordingWatchdog {
    fn stroke(&mut self) {
        self.strokes.fetch_add(1, Ordering::Relaxed);
    }
}

/// Per-task consecutive overrun tracker.
#[derive(Debug)]
pub struct SoftwareWatchdog {
    overrun_limit: u32,
    consecutive: Vec<u32>,
}

impl SoftwareWatchdog {
    /// `overrun_limit` consecutive overruns (config default 2) trip FAULT.
    #[must_use]
    pub fn new(n_tasks: usize, overrun_limit: u32) -> Self {
        Self {
            overrun_limit: overrun_limit.max(1),
            consecutive: vec![0; n_tasks],
        }
    }

    /// Record an invocation duration.
    ///
    /// An overrun is `duration_us >= period_ms * 1000` so a 50 ms injection
    /// on a 50 ms task counts (PR-07 acceptance).
    ///
    /// Returns `true` when this task has now reached the consecutive limit.
    pub fn note(&mut self, task: usize, duration_us: u64, period_ms: u32) -> bool {
        let budget = u64::from(period_ms).saturating_mul(1000);
        let Some(slot) = self.consecutive.get_mut(task) else {
            return false;
        };
        if duration_us >= budget {
            *slot = slot.saturating_add(1);
            *slot >= self.overrun_limit
        } else {
            *slot = 0;
            false
        }
    }

    /// Consecutive overrun count for `task`.
    #[must_use]
    pub fn consecutive(&self, task: usize) -> u32 {
        self.consecutive.get(task).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_overruns_trip() {
        let mut wd = SoftwareWatchdog::new(1, 2);
        assert!(!wd.note(0, 50_000, 50));
        assert!(wd.note(0, 50_000, 50));
    }

    #[test]
    fn in_time_resets() {
        let mut wd = SoftwareWatchdog::new(1, 2);
        assert!(!wd.note(0, 50_000, 50));
        assert!(!wd.note(0, 1_000, 50));
        assert_eq!(wd.consecutive(0), 0);
        assert!(!wd.note(0, 50_000, 50));
    }

    #[test]
    fn recording_counts() {
        let mut wd = RecordingWatchdog::new();
        wd.stroke();
        wd.stroke();
        assert_eq!(wd.count(), 2);
    }
}
