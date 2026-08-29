//! Runtime glue errors (arm/activate). Validation never maps to FAULT.

use plc_package::PackageError;
use plc_retain::RetainError;
use plc_scan::ScanError;
use plc_types::PlcError;
use plc_vm::VmError;
use thiserror::Error;

/// Errors from upload / arm / activate. None of these enter FAULT by themselves.
#[derive(Debug, Error)]
pub enum RuntimeError {
    /// Package parse, signature, or IR verify failed.
    #[error(transparent)]
    Package(#[from] PackageError),
    /// Retain remap refused (incompatible type without force).
    #[error(transparent)]
    Retain(#[from] RetainError),
    /// Scan engine refused the operation (phase / image).
    #[error(transparent)]
    Scan(#[from] ScanError),
    /// VM load failed.
    #[error(transparent)]
    Vm(#[from] VmError),
    /// Operation not allowed in the current phase (e.g. upload while swapping).
    #[error("conflict: {context}")]
    Conflict {
        /// Why the operation collided.
        context: String,
    },
    /// Semantic arm failure (missing task, ABI, …).
    #[error("arm: {0}")]
    Arm(String),
    /// Named resource is not in the current/armed dictionary or image.
    #[error("not found: {0}")]
    NotFound(String),
    /// Tag type or force target refused.
    #[error("bad request: {0}")]
    BadRequest(String),
}

impl RuntimeError {
    pub(crate) fn conflict(context: impl Into<String>) -> Self {
        Self::Conflict {
            context: context.into(),
        }
    }

    pub(crate) fn arm(msg: impl Into<String>) -> Self {
        Self::Arm(msg.into())
    }

    pub(crate) fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    pub(crate) fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }
}

impl From<RuntimeError> for PlcError {
    fn from(value: RuntimeError) -> Self {
        match value {
            RuntimeError::Package(e) => Self::Package(e.to_string()),
            RuntimeError::Retain(e) => Self::Package(e.to_string()),
            RuntimeError::Scan(e) => e.into(),
            RuntimeError::Vm(e) => Self::Scan(e.to_string()),
            RuntimeError::Conflict { context } => Self::InvalidState { context },
            RuntimeError::Arm(msg) => Self::Package(msg),
            RuntimeError::NotFound(msg) | RuntimeError::BadRequest(msg) => Self::Scan(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_maps_to_package_not_fault_category() {
        let err = RuntimeError::from(PackageError::BadMagic);
        let plc = PlcError::from(err);
        assert_eq!(plc.category(), "package");
    }
}
