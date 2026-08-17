//! Scan status snapshots for REST / diagnostics (PR-12).

use plc_types::{OperatingMode, ProgramPhase};

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
}
