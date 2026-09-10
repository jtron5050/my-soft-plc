//! Dual-buffer program load and epoch activate (architecture PR-10, KD-4a).
//!
//! Non-RT glue: upload → validate → arm (shadow retain) → request activate.
//! The scan engine performs the quiet-point join, skip rule, and install CS.

#![deny(unsafe_code)]

mod catalog;
mod error;
mod loader;
mod scan_thread;

pub use catalog::catalog_from_tags;
pub use error::RuntimeError;
pub use loader::{
    ArmContext, ArmReport, PreparedArm, ProgramInfo, RetainSnapshot, Runtime, RuntimeConfig,
    TagView,
};
pub use plc_package::{RestartPolicy, TagEntry, TagKind, VerifyPolicy};
pub use plc_retain::MapReport;
pub use plc_scan::{
    ActivateRequest, ArmedProgram, InstallOutcome, OutputRestartPolicy, RetainCopy, ScanEngine,
    ScanIo, ScanPlan,
};
pub use plc_types::{OperatingMode, ProgramPhase};
pub use scan_thread::spawn_scan_thread;
