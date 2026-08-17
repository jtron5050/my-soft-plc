//! Package parse, validate, and signature errors.

use thiserror::Error;

/// Errors from framing, JSON/JCS, signature, or manifest/`spbc` checks.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PackageError {
    /// Entire buffer exceeds [`crate::MAX_PACKAGE_BYTES`].
    #[error("package exceeds 8 MiB limit ({0} bytes)")]
    TooLarge(usize),
    /// Magic is not `SPKG` and is not classified as CBOR.
    #[error("bad package magic")]
    BadMagic,
    /// CBOR (self-describe tag or non-JSON high-bit payload). Package major 1 is JSON only.
    #[error("CBOR is not accepted in package major 1")]
    CborRejected,
    /// `pkg_version` is not 1.
    #[error("unsupported package version {0} (expected 1)")]
    UnsupportedVersion(u16),
    /// Buffer ended before a complete frame, manifest, section, or signature.
    #[error("truncated package: {0}")]
    Truncated(&'static str),
    /// Bytes remain after the 64-byte signature.
    #[error("trailing bytes after package signature")]
    TrailingBytes,
    /// JSON parse failure (duplicate keys, comments, non-UTF-8, type errors).
    #[error("package JSON: {0}")]
    Json(String),
    /// JCS serialization failed.
    #[error("JCS canonicalization: {0}")]
    Jcs(String),
    /// Manifest semantic error (semver, program id, empty tasks, unknown type).
    #[error("manifest: {0}")]
    Manifest(String),
    /// Ed25519 verification failed, or no trust anchors were supplied.
    #[error("signature verification failed")]
    Signature,
    /// Signature is the all-zero sentinel and `require_signature` is set.
    #[error("package is unsigned")]
    Unsigned,
    /// `section_count` is 0 (parse) or not 1 (validate, package major 1).
    #[error("invalid spbc section count {0} (package major 1 requires 1)")]
    SectionCount(u32),
    /// `spbc` blob could not be parsed.
    #[error(transparent)]
    Spbc(#[from] plc_ir::IrError),
    /// IR verifier rejected the module.
    #[error(transparent)]
    Verify(#[from] plc_ir::VerifyError),
    /// Manifest image/entry fields disagree with the `spbc` header.
    #[error("manifest disagrees with spbc: {0}")]
    ManifestSpbcMismatch(String),
    /// Stored `compatibility_hash` does not match the recomputed value.
    #[error("compatibility_hash mismatch")]
    CompatibilityHash,
    /// `id` is not a single `[A-Za-z0-9._-]+` path segment.
    #[error("invalid program id: {0}")]
    InvalidProgramId(String),
}

impl PackageError {
    pub(crate) fn json(msg: impl Into<String>) -> Self {
        Self::Json(msg.into())
    }

    pub(crate) fn jcs(msg: impl Into<String>) -> Self {
        Self::Jcs(msg.into())
    }

    pub(crate) fn manifest(msg: impl Into<String>) -> Self {
        Self::Manifest(msg.into())
    }

    pub(crate) fn mismatch(msg: impl Into<String>) -> Self {
        Self::ManifestSpbcMismatch(msg.into())
    }
}
