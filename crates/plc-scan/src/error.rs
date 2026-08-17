//! Scan-engine errors.

use plc_io::IoError;
use plc_types::PlcError;
use plc_vm::VmError;
use thiserror::Error;

/// Errors from constructing or stepping the scan engine.
#[derive(Debug, Error)]
pub enum ScanError {
    /// Task table or policy refused at construction.
    #[error("scan config: {0}")]
    Config(String),
    /// Process image / VM / driver slot counts disagree.
    #[error("image mismatch: {reason}")]
    ImageMismatch {
        /// Why the images are incompatible.
        reason: String,
    },
    /// IR VM load or execution failure.
    #[error(transparent)]
    Vm(#[from] VmError),
    /// Driver poll/apply failure that the engine treats as hard (rare).
    #[error(transparent)]
    Io(#[from] IoError),
    /// Mode or phase transition refused.
    #[error("invalid state: {context}")]
    InvalidState {
        /// Human-readable context (e.g. "SIM from RUN").
        context: String,
    },
}

impl ScanError {
    /// Config helper.
    #[must_use]
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    /// Image-mismatch helper.
    #[must_use]
    pub fn image_mismatch(reason: impl Into<String>) -> Self {
        Self::ImageMismatch {
            reason: reason.into(),
        }
    }

    /// Invalid-state helper.
    #[must_use]
    pub fn invalid_state(context: impl Into<String>) -> Self {
        Self::InvalidState {
            context: context.into(),
        }
    }
}

impl From<ScanError> for PlcError {
    fn from(value: ScanError) -> Self {
        match value {
            ScanError::Config(msg) => Self::Scan(msg),
            ScanError::ImageMismatch { reason } => Self::Scan(reason),
            ScanError::Vm(e) => Self::Scan(e.to_string()),
            ScanError::Io(e) => Self::Io(e.to_string()),
            ScanError::InvalidState { context } => Self::InvalidState { context },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_into_plc_error() {
        let err = ScanError::invalid_state("SIM from RUN");
        let plc = PlcError::from(err);
        assert_eq!(plc.category(), "invalid_state");
    }
}
