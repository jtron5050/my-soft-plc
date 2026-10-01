//! Modbus TCP I/O driver.
//!
//! The poll loop runs on one non-RT thread. [`ModbusBridge`] implements
//! [`plc_io::IoDriver`] by copying snapshots, so the scan thread never waits
//! on a socket (KD-5a).

#![forbid(unsafe_code)]

mod address;
mod codec;
mod convert;
mod driver;
mod validate;

pub use driver::ModbusBridge;
pub use validate::{validate_map, ModbusPlan, PlannedModule, PlannedPoint};
