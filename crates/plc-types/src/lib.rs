//! Shared types and errors for the soft PLC runtime.
//!
//! This crate is intentionally dependency-light so both RT-path crates
//! (`plc-scan`, `plc-vm`, …) and non-RT services can share enums and error
//! kinds without pulling tokio or network stacks.

#![forbid(unsafe_code)]

mod error;
mod image;
mod mode;
mod quality;
mod rt_path;

pub use error::{PlcError, PlcResult};
pub use image::ImageRegion;
pub use mode::{OperatingMode, ProgramPhase};
pub use quality::Quality;
pub use rt_path::{RT_FORBIDDEN_CRATE_HINTS, RT_PATH_CRATES};
