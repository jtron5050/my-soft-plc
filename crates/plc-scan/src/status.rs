//! Scan status snapshots for REST / diagnostics (PR-12, scan stats in PR-18).

use plc_types::{OperatingMode, ProgramPhase};

/// Upper bounds (microseconds) of the finite scan-duration histogram buckets.
/// The last stored count is `+Inf` (greater than the last bound).
pub const DURATION_BUCKET_BOUNDS_US: [u64; 9] = [
    100, 500, 1_000, 2_000, 5_000, 10_000, 20_000, 50_000, 100_000,
];

/// Finite bounds plus the `+Inf` bucket.
pub const DURATION_BUCKETS: usize = DURATION_BUCKET_BOUNDS_US.len() + 1;

/// Index of the histogram bucket that contains `us` (`<=` each bound).
#[must_use]
pub fn duration_bucket(us: u64) -> usize {
    for (i, bound) in DURATION_BUCKET_BOUNDS_US.iter().enumerate() {
        if us <= *bound {
            return i;
        }
    }
    DURATION_BUCKET_BOUNDS_US.len()
}

/// Per-task timing as last observed by the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskTiming {
    /// Task name (`fast`, `main`, …).
    pub name: String,
    /// Configured period.
    pub period_ms: u32,
    /// Last invocation duration (microseconds).
    pub last_us: u64,
    /// Max invocation duration (microseconds).
    pub max_us: u64,
    /// Integer mean of invocation durations (`sum_us / samples`, 0 if none).
    pub avg_us: u64,
    /// Sum of invocation durations (microseconds).
    pub sum_us: u64,
    /// Invocations included in `sum_us` / the histogram.
    pub samples: u64,
    /// Raw (not cumulative) histogram counts. Last index is `+Inf`.
    pub duration_buckets: [u64; DURATION_BUCKETS],
    /// Lifetime overrun count.
    pub overruns: u32,
    /// Consecutive overruns (cleared on an in-time scan).
    pub consecutive_overruns: u32,
}

/// Point-in-time engine status (copied off atomics / locals).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanStatusSnapshot {
    /// Operator mode.
    pub mode: OperatingMode,
    /// Program phase (Idle until PR-10).
    pub phase: ProgramPhase,
    /// Per-task timing.
    pub tasks: Vec<TaskTiming>,
    /// Telemetry ring drops.
    pub telemetry_drops: u64,
    /// Rejected mode requests.
    pub mode_rejected: u64,
    /// Module quality is Bad.
    pub io_degraded: bool,
    /// At least one successful RUN/SIM invocation has completed.
    pub first_run_complete: bool,
    /// Current program id, if a package has been activated.
    pub current_program_id: Option<String>,
    /// Armed (buffer B) program id, if any.
    pub armed_program_id: Option<String>,
    /// How many activate attempts were deferred (deadline miss).
    pub activate_deferred_count: u64,
    /// Last install attempt ended in `activate_deferred`.
    pub last_activate_deferred: bool,
}
