//! Retain store errors.

use std::io;
use std::path::PathBuf;

use thiserror::Error;

/// Errors from layout, codec, map, or NV I/O.
///
/// Boot [`crate::RetainStore::load`] does **not** return corruption as an
/// error — it cold-starts and reports via [`crate::LoadReport`].
#[derive(Debug, Error)]
pub enum RetainError {
    /// Filesystem failure.
    #[error("retain I/O error at {path}: {source}")]
    Io {
        /// Path involved.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// `program_id` is not a single `[A-Za-z0-9._-]+` path segment.
    #[error("invalid retain program id: {0}")]
    InvalidProgramId(String),
    /// Image or destination length does not match `layout.retain_size`.
    #[error("retain image size {actual} != layout retain_size {expected}")]
    ImageSize {
        /// Expected byte count.
        expected: u32,
        /// Actual slice length.
        actual: usize,
    },
    /// Same path, incompatible type, and `force_retain_incompat` is false.
    #[error("incompatible retain types: {}", names.join(", "))]
    Incompatible {
        /// Symbol paths with a type mismatch.
        names: Vec<String>,
    },
    /// Layout validation failed.
    #[error(transparent)]
    Layout(#[from] plc_ir::IrError),
    /// Symbolic payload could not be decoded.
    #[error("retain codec: {0}")]
    Codec(String),
}

impl RetainError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    pub(crate) fn codec(msg: impl Into<String>) -> Self {
        Self::Codec(msg.into())
    }
}
