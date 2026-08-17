//! IR parse / assemble / verify errors.

use thiserror::Error;

/// Assembler, binary parse, or encode errors.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum IrError {
    /// Text assembly syntax error.
    #[error("spasm error at line {line}: {message}")]
    Asm {
        /// 1-based line number.
        line: usize,
        /// Human-readable message.
        message: String,
    },
    /// Binary `spbc` framing error.
    #[error("spbc error: {0}")]
    Spbc(String),
    /// Unknown opcode or primitive.
    #[error("unknown {what}: {name}")]
    Unknown {
        /// Kind of symbol.
        what: &'static str,
        /// Symbol text.
        name: String,
    },
    /// Retain layout rejected (duplicate name, overlap, or out of bounds).
    #[error("retain layout: {0}")]
    RetainLayout(String),
}

/// Verifier rule violations (checklist A.6).
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum VerifyError {
    /// Named rule failed.
    #[error("verify rule {rule}: {message}")]
    Rule {
        /// Checklist rule number (1–10) or 0 for general.
        rule: u8,
        /// Detail.
        message: String,
    },
}

impl VerifyError {
    /// Convenience constructor.
    #[must_use]
    pub fn rule(rule: u8, message: impl Into<String>) -> Self {
        Self::Rule {
            rule,
            message: message.into(),
        }
    }
}
