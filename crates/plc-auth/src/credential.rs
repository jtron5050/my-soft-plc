//! Credentials, SHA-256 hashing, and hex helpers.

use core::fmt;

use sha2::{Digest, Sha256};

use crate::AuthError;

/// Presented credentials. Bearer secrets are never shown in [`Debug`].
#[derive(Clone, PartialEq, Eq)]
pub enum Credential {
    /// No credentials (anonymous when auth is not required).
    None,
    /// Opaque bearer token (the secret, not the hash).
    Bearer(String),
    /// Client certificate identity extracted by the TLS terminator.
    ClientCert {
        /// SHA-256 of the raw client certificate DER.
        fingerprint_sha256: [u8; 32],
        /// Subject CN for logging only — **not** an auth factor in v1.
        subject_cn: Option<String>,
    },
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Bearer(_) => write!(f, "Bearer(***)"),
            Self::ClientCert {
                fingerprint_sha256,
                subject_cn,
            } => f
                .debug_struct("ClientCert")
                .field("fingerprint_sha256", &hex_encode(fingerprint_sha256))
                .field("subject_cn", subject_cn)
                .finish(),
        }
    }
}

/// SHA-256 of `secret` (bearer token bytes or cert DER).
#[must_use]
pub fn hash_secret(secret: &[u8]) -> [u8; 32] {
    Sha256::digest(secret).into()
}

/// Lowercase hex of `bytes`.
#[must_use]
pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

/// Decode 64 hex characters (upper or lower) into 32 bytes.
pub fn hex_decode_32(s: &str) -> Result<[u8; 32], AuthError> {
    let t = s.trim();
    if t.len() != 64 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(AuthError::Config(
            "expected 32 hex bytes (64 hex characters)".into(),
        ));
    }
    let raw = t.as_bytes();
    let mut out = [0u8; 32];
    for i in 0..32 {
        let hi = hex_val(raw[i * 2])?;
        let lo = hex_val(raw[i * 2 + 1])?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_val(b: u8) -> Result<u8, AuthError> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(AuthError::Config("invalid hex digit".into())),
    }
}

/// Constant-time equality for equal-length slices. Different lengths return
/// `false` without comparing bytes.
#[must_use]
pub(crate) fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut d = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        d |= x ^ y;
    }
    d == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ct_eq_accepts_equal() {
        assert!(ct_eq(b"abcd", b"abcd"));
        assert!(!ct_eq(b"abcd", b"abce"));
    }

    #[test]
    fn ct_eq_rejects_length_mismatch() {
        assert!(!ct_eq(b"abc", b"abcd"));
        assert!(!ct_eq(b"", b"x"));
    }

    #[test]
    fn hex_round_trip() {
        let h = hash_secret(b"secret");
        let s = hex_encode(&h);
        assert_eq!(s.len(), 64);
        assert_eq!(hex_decode_32(&s).unwrap(), h);
        assert_eq!(hex_decode_32(&s.to_ascii_uppercase()).unwrap(), h);
    }

    #[test]
    fn bearer_debug_redacts_secret() {
        let dbg = format!("{:?}", Credential::Bearer("super-secret".into()));
        assert!(!dbg.contains("super-secret"));
        assert!(dbg.contains("Bearer"));
    }
}
