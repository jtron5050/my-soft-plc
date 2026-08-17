//! Cooperative scan scheduler for the soft PLC runtime.
//!
//! Architecture PR-07: single-thread I→L→Q tasks, STOP/RUN/FAULT/SIM,
//! software overrun watchdog, [`TelemetrySource`] SPSC, dirty-retain signal,
//! and epoch hooks for PR-10.

#![forbid(unsafe_code)]

mod clock;
mod convert;
mod engine;
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
pub use error::ScanError;
pub use hooks::EpochHooks;
pub use mode::{ModeRequest, ScanHandle};
pub use retain_signal::{RetainDirtyEvent, RetainDirtyWatch};
pub use status::{ScanStatusSnapshot, TaskTiming};
pub use telemetry::{TelemetrySample, TelemetrySource};
pub use watchdog::{HardwareWatchdog, NullWatchdog, RecordingWatchdog};
