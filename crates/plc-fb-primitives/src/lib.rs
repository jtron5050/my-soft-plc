//! Native function-block primitives for the soft PLC IR VM.
//!
//! These are **not** user-downloadable code: they live in the runtime and are
//! invoked via `CALL_FB` with a primitive id (architecture PR-05, KD-12).
//!
//! # Timebase (KD-16)
//!
//! Timers use a monotonic `now_ms: u64` supplied by the scan engine (sampled
//! once per task invocation). Wall-clock / NTP must never feed TON/TOF/TP.

#![forbid(unsafe_code)]

mod counter;
mod dispatch;
mod edge;
mod latch;
mod pid;
mod timer;

pub use counter::{Ctd, Ctu};
pub use dispatch::{
    call_primitive, PrimitiveCallError, PrimitiveOutputs, PrimitiveStore, StackValue,
};
pub use edge::{FTrig, RTrig};
pub use latch::{Rs, Sr};
pub use pid::Pid;
pub use timer::{Tof, Ton, Tp};

/// ABI version for `primitive_abi` / package compatibility hashing.
///
/// Bump when instance layouts or CALL_FB input/output counts change.
pub const PRIMITIVE_ABI: u32 = 1;
