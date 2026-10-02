//! Linux GPIO DI/DO driver.
//!
//! [`GpioDriver`] implements [`plc_io::IoDriver`] on the scan thread.
//! `poll_inputs` and `apply_outputs` issue one GPIO character-device ioctl
//! per module and do not allocate. Sockets and worker threads stay out of
//! this crate (KD-5a).

#![deny(unsafe_code)]

mod driver;
mod linux;
mod validate;

pub use driver::GpioDriver;
pub use validate::{
    validate_map, Bias, Drive, GpioPlan, LineDirection, PlannedModule, PlannedPoint,
};
