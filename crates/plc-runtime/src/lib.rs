//! Dual-buffer program load and epoch activate (architecture PR-10, KD-4a).
//!
//! Non-RT glue: upload → validate → arm (shadow retain) → request activate.
//! The scan engine performs the quiet-point join, skip rule, and install CS.

#![forbid(unsafe_code)]

mod error;
mod loader;

pub use error::RuntimeError;
pub use loader::{ArmReport, Runtime, RuntimeConfig};
pub use plc_package::{RestartPolicy, VerifyPolicy};
pub use plc_retain::MapReport;
pub use plc_scan::{
    ActivateRequest, ArmedProgram, InstallOutcome, OutputRestartPolicy, RetainCopy, ScanEngine,
    ScanIo, ScanPlan,
};
pub use plc_types::{OperatingMode, ProgramPhase};
