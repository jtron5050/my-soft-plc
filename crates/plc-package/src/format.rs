//! `.spkg` binary framing (magic, length-prefixed JSON, `spbc` sections, Ed25519).

use plc_ir::{parse_spbc, IrModule};

use crate::error::PackageError;
use crate::jcs::looks_like_cbor;
use crate::manifest::Manifest;

/// `SPKG` magic.
pub const SPKG_MAGIC: &[u8; 4] = b"SPKG";
/// Package major 1.
pub const SPKG_VERSION: u16 = 1;
/// Architecture upload / parse cap.
pub const MAX_PACKAGE_BYTES: usize = 8 * 1024 * 1024;
/// Ed25519 signature length.
pub const SIGNATURE_LEN: usize = 64;

/// Minimum bytes for magic + version + empty-manifest length field.
const MIN_HEADER: usize = 4 + 2 + 4;

/// Parsed package (framing + JSON/JCS + `spbc` modules). Crypto is not checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPackage {
    /// Typed manifest.
    pub manifest: Manifest,
    /// Manifest bytes as stored (not necessarily JCS).
    pub manifest_raw: Vec<u8>,
    /// RFC 8785 JCS of the parsed manifest object.
    pub manifest_canonical: Vec<u8>,
    /// Raw `spbc` section blobs in package order.
    pub sections: Vec<Vec<u8>>,
    /// Parsed modules (same order as [`Self::sections`]).
    pub modules: Vec<IrModule>,
    /// Trailing 64-byte Ed25519 signature (may be all-zero).
    pub signature: [u8; SIGNATURE_LEN],
}

/// Parse a complete `.spkg` buffer. Does not verify the signature or IR rules.
pub fn parse_spkg(bytes: &[u8]) -> Result<ParsedPackage, PackageError> {
    if bytes.len() > MAX_PACKAGE_BYTES {
        return Err(PackageError::TooLarge(bytes.len()));
    }
    if bytes.len() < MIN_HEADER {
        return if looks_like_cbor(bytes) {
            Err(PackageError::CborRejected)
        } else {
            Err(PackageError::Truncated("header"))
        };
    }
    if bytes[0..4] != *SPKG_MAGIC {
        return if looks_like_cbor(bytes) {
            Err(PackageError::CborRejected)
        } else {
            Err(PackageError::BadMagic)
        };
    }
    let version = u16::from_le_bytes(bytes[4..6].try_into().unwrap());
    if version != SPKG_VERSION {
        return Err(PackageError::UnsupportedVersion(version));
    }
    let manifest_len = u32::from_le_bytes(bytes[6..10].try_into().unwrap()) as usize;
    let manifest_start = 10usize;
    let manifest_end = manifest_start
        .checked_add(manifest_len)
        .ok_or(PackageError::Truncated("manifest length overflow"))?;
    if manifest_end > bytes.len() {
        return Err(PackageError::Truncated("manifest"));
    }
    let manifest_raw = bytes[manifest_start..manifest_end].to_vec();
    let (manifest, manifest_canonical) = Manifest::from_json_bytes(&manifest_raw)?;

    let mut off = manifest_end;
    if off + 4 > bytes.len() {
        return Err(PackageError::Truncated("section_count"));
    }
    let section_count = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
    off += 4;
    if section_count == 0 {
        return Err(PackageError::SectionCount(0));
    }

    let mut sections = Vec::with_capacity(section_count as usize);
    for _ in 0..section_count {
        if off + 4 > bytes.len() {
            return Err(PackageError::Truncated("section length"));
        }
        let section_len = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
        off += 4;
        let end = off
            .checked_add(section_len)
            .ok_or(PackageError::Truncated("section length overflow"))?;
        if end > bytes.len() {
            return Err(PackageError::Truncated("section body"));
        }
        sections.push(bytes[off..end].to_vec());
        off = end;
    }

    if off + SIGNATURE_LEN > bytes.len() {
        return Err(PackageError::Truncated("signature"));
    }
    if off + SIGNATURE_LEN < bytes.len() {
        return Err(PackageError::TrailingBytes);
    }
    let mut signature = [0u8; SIGNATURE_LEN];
    signature.copy_from_slice(&bytes[off..off + SIGNATURE_LEN]);

    let mut modules = Vec::with_capacity(sections.len());
    for section in &sections {
        modules.push(parse_spbc(section)?);
    }

    Ok(ParsedPackage {
        manifest,
        manifest_raw,
        manifest_canonical,
        sections,
        modules,
        signature,
    })
}

/// Serialize framing. `manifest_json` is stored as-is (builder writes JCS).
pub fn write_spkg(
    manifest_json: &[u8],
    sections: &[Vec<u8>],
    signature: &[u8; SIGNATURE_LEN],
) -> Result<Vec<u8>, PackageError> {
    if sections.is_empty() {
        return Err(PackageError::SectionCount(0));
    }
    let mut out = Vec::new();
    out.extend_from_slice(SPKG_MAGIC);
    out.extend_from_slice(&SPKG_VERSION.to_le_bytes());
    let manifest_len = u32::try_from(manifest_json.len())
        .map_err(|_| PackageError::json("manifest longer than u32"))?;
    out.extend_from_slice(&manifest_len.to_le_bytes());
    out.extend_from_slice(manifest_json);
    let count = u32::try_from(sections.len()).map_err(|_| PackageError::SectionCount(u32::MAX))?;
    out.extend_from_slice(&count.to_le_bytes());
    for section in sections {
        let len = u32::try_from(section.len())
            .map_err(|_| PackageError::Truncated("section longer than u32"))?;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(section);
    }
    out.extend_from_slice(signature);
    if out.len() > MAX_PACKAGE_BYTES {
        return Err(PackageError::TooLarge(out.len()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_rejected_before_parse() {
        let n = MAX_PACKAGE_BYTES + 1;
        let bytes = vec![0u8; n];
        assert_eq!(parse_spkg(&bytes).unwrap_err(), PackageError::TooLarge(n));
    }

    #[test]
    fn cbor_magic_rejected() {
        assert_eq!(
            parse_spkg(&[0xD9, 0xD9, 0xF7, 0xA0]).unwrap_err(),
            PackageError::CborRejected
        );
    }

    #[test]
    fn bad_version() {
        let mut bytes = Vec::from(*SPKG_MAGIC);
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes()); // section_count
        bytes.extend_from_slice(&[0u8; SIGNATURE_LEN]);
        assert_eq!(
            parse_spkg(&bytes).unwrap_err(),
            PackageError::UnsupportedVersion(2)
        );
    }
}
