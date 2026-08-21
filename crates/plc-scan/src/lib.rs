//! Cooperative scan scheduler for the soft PLC runtime.
//!
//! Architecture PR-07 / PR-10: single-thread I→L→Q tasks, STOP/RUN/FAULT/SIM,
//! software overrun watchdog, [`TelemetrySource`] SPSC, dirty-retain signal,
//! and program-epoch (KD-4a) dual-buffer activate.

#![forbid(unsafe_code)]

mod clock;
mod convert;
mod engine;
mod epoch;
mod error;
mod hooks;
mod mode;
mod retain_signal;
mod spsc;
mod status;
mod telemetry;
mod watchdog;

pub use clock::{MonotonicClock, ScanClock, VirtualClock};
pub use engine::{ScanEngine, ScanIo, ScanPlan, StepOutcome, TaskPlan, DEFAULT_TELEMETRY_CAPACITY};
pub use epoch::{ActivateRequest, ArmedProgram, InstallOutcome, OutputRestartPolicy, RetainCopy};
pub use error::ScanError;
pub use hooks::EpochHooks;
pub use mode::{ModeRequest, ScanHandle};
pub use retain_signal::{RetainDirtyEvent, RetainDirtyWatch};
pub use status::{ScanStatusSnapshot, TaskTiming};
pub use telemetry::{TelemetrySample, TelemetrySource};
pub use watchdog::{HardwareWatchdog, NullWatchdog, RecordingWatchdog};
