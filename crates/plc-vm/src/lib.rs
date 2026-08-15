//! IR v0.1 virtual machine.
//!
//! Loads a verified [`plc_ir::IrModule`], pre-allocates image segments and
//! primitive instance pools at arm time, and executes task / FB entries with
//! **no heap allocation in the run loop** (architecture PR-06, KD-13).

#![forbid(unsafe_code)]

mod error;
mod exec;
mod load;
mod memory;
mod value;

#[cfg(test)]
#[path = "exec_tests.rs"]
mod exec_tests;

pub use error::VmError;
pub use exec::{ExecResult, Vm};
pub use load::VmConfig;
pub use value::VmValue;

/// Maximum abstract-machine stack depth (Appendix A.6 rule 2).
pub const MAX_STACK: usize = 256;
/// Maximum user-FB call depth (Appendix A.5 / A.6 rule 5).
pub const MAX_CALL_DEPTH: usize = 32;
