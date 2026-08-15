//! VM load and runtime errors.

use thiserror::Error;

/// Errors from loading or executing IR.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum VmError {
    /// Module failed verification.
    #[error("IR verify failed: {0}")]
    Verify(String),
    /// Named entry not found.
    #[error("unknown entry '{0}'")]
    UnknownEntry(String),
    /// Operand stack underflow.
    #[error("stack underflow at pc={pc}")]
    StackUnderflow {
        /// Byte PC of the faulting instruction.
        pc: usize,
    },
    /// Operand stack overflow (> 256).
    #[error("stack overflow at pc={pc}")]
    StackOverflow {
        /// Byte PC.
        pc: usize,
    },
    /// Call depth exceeded 32.
    #[error("call depth exceeded at pc={pc}")]
    CallDepth {
        /// Byte PC.
        pc: usize,
    },
    /// RET with empty call stack.
    #[error("RET outside user FB at pc={pc}")]
    RetOutsideFb {
        /// Byte PC.
        pc: usize,
    },
    /// Memory / image index out of bounds.
    #[error("memory bounds at pc={pc}: {detail}")]
    Bounds {
        /// Byte PC.
        pc: usize,
        /// Detail message.
        detail: String,
    },
    /// Type mismatch for an arithmetic or logic op.
    #[error("type error at pc={pc}: {detail}")]
    Type {
        /// Byte PC.
        pc: usize,
        /// Detail message.
        detail: String,
    },
    /// Unknown opcode or bad decode mid-run (should not happen post-verify).
    #[error("decode error at pc={pc}: {detail}")]
    Decode {
        /// Byte PC.
        pc: usize,
        /// Detail message.
        detail: String,
    },
    /// Primitive CALL_FB failure.
    #[error("primitive call at pc={pc}: {detail}")]
    Primitive {
        /// Byte PC.
        pc: usize,
        /// Detail message.
        detail: String,
    },
    /// User FB id has no registered entry PC.
    #[error("unknown user FB id {0}")]
    UnknownUserFb(u32),
    /// Instruction budget exhausted (runaway / missing HALT).
    #[error("instruction budget exhausted ({0})")]
    Budget(u64),
}
