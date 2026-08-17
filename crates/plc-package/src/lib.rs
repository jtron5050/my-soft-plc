//! Signed program package format (architecture PR-09).
//!
//! `.spkg` v1 is a little-endian container:
//!
//! ```text
//! magic "SPKG" (4)
//! version u16 = 1
//! manifest_len u32
//! manifest UTF-8 JSON   // re-canonicalized with RFC 8785 JCS for the signature
//! section_count u32     // validate requires 1
//! sections: [ len u32, spbc bytes ] × section_count
//! signature [u8; 64]    // Ed25519 over SHA-256(JCS ‖ bytecode…)
//! ```
//!
//! This crate is **non-RT**. Dual-buffer arm/activate lives in PR-10.

#![forbid(unsafe_code)]

mod compat;
mod error;
mod format;
mod jcs;
mod manifest;
mod pack;
mod sign;
mod validate;

pub use compat::{compatibility_preimage, compute_compatibility_hash, hex_decode_n, hex_encode};
pub use ed25519_dalek::{SigningKey, VerifyingKey};
pub use error::PackageError;
pub use format::{
    parse_spkg, write_spkg, ParsedPackage, MAX_PACKAGE_BYTES, SIGNATURE_LEN, SPKG_MAGIC,
    SPKG_VERSION,
};
pub use jcs::{parse_strict_json, StrictValue};
pub use manifest::{
    validate_program_id, IrTypeName, Manifest, ManifestRetainSymbol, RestartPolicy, TagEntry,
    TagKind,
};
pub use pack::PackageBuilder;
pub use sign::{
    check_policy, sign, signing_key_from_seed, signing_preimage, verify_signature, VerifyPolicy,
};
pub use validate::validate;
