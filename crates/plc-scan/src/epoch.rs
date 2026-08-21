//! Dual-buffer arm slot and hot-swap restart policy (KD-4a).

use plc_vm::Vm;

/// Output restart policy after a successful activate (manifest `restart_policy`).
///
/// Eligibility for [`Self::Bumpless`] is resolved at arm time: the runtime
/// must pass [`Self::SafeReset`] when `compatibility_hash` does not match
/// the running program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputRestartPolicy {
    /// Force `safe_state` at install, then the program drives `%Q`.
    SafeReset,
    /// Hold last `%Q` through each task's first post-activate invocation.
    Bumpless,
}

/// One precomputed retain blit for the activate critical section.
///
/// Built on the non-RT arm path from the keep-set of the symbol-path map.
/// The CS copies bytes + tags only — no name walks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetainCopy {
    /// Byte offset in the **current** (old) retain segment.
    pub src_offset: usize,
    /// Byte offset in the **armed** (new) retain segment.
    pub dst_offset: usize,
    /// Byte width of the kept symbol.
    pub len: usize,
}

/// Buffer B: a validated program ready for epoch activate.
pub struct ArmedProgram {
    /// Loaded VM. Non-retain is cold-reset at [`crate::ScanEngine::arm`]; retain
    /// shadow is installed by the loader before arm. CS only blits keep-set bytes.
    pub vm: Vm,
    /// Program id (package manifest).
    pub program_id: String,
    /// Manifest `compatibility_hash` (lowercase hex).
    pub compatibility_hash: String,
    /// IR entry symbol for each scan task (parallel to [`crate::ScanPlan::tasks`]).
    pub task_entries: Vec<String>,
    /// Restart policy **after** bumpless eligibility is applied.
    pub restart_policy: OutputRestartPolicy,
    /// Arm-time keep-set copies applied from live retain at CS time.
    pub retain_copies: Vec<RetainCopy>,
}

/// Result of [`crate::ScanEngine::request_activate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivateRequest {
    /// Swap will run at the next highest-priority quiet boundary.
    Pending,
    /// Already current `id` + `compatibility_hash`; armed buffer dropped.
    NoOp,
}

/// Outcome of an install attempt inside `step` / `run_due`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    /// No activate request, or not at a quiet highest-priority boundary.
    Idle,
    /// Pointer-swing committed; new program is current.
    Installed,
    /// Install work exceeded `min_task_period`; remain armed (not FAULT).
    Deferred,
    /// Invariant failure during install; engine entered FAULT.
    Faulted,
}
