//! SHA-256 signing preimage and Ed25519 PureEdDSA.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::error::PackageError;

/// Policy for [`crate::validate`].
#[derive(Debug, Clone, Copy)]
pub struct VerifyPolicy<'a> {
    /// When true, all-zero signatures and failed Ed25519 checks are errors.
    pub require_signature: bool,
    /// Trust anchors; success if **any** key verifies.
    pub public_keys: &'a [VerifyingKey],
}

impl VerifyPolicy<'static> {
    /// Dev / tests: skip crypto entirely.
    #[must_use]
    pub const fn unsigned() -> Self {
        Self {
            require_signature: false,
            public_keys: &[],
        }
    }
}

impl<'a> VerifyPolicy<'a> {
    /// Production-style: require a signature that matches one of `public_keys`.
    #[must_use]
    pub const fn required(public_keys: &'a [VerifyingKey]) -> Self {
        Self {
            require_signature: true,
            public_keys,
        }
    }
}

/// `SHA-256(manifest_canonical ‖ bytecode_0 ‖ bytecode_1 ‖ …)`.
#[must_use]
pub fn signing_preimage(canonical_manifest: &[u8], sections: &[impl AsRef<[u8]>]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(canonical_manifest);
    for section in sections {
        hasher.update(section.as_ref());
    }
    hasher.finalize().into()
}

/// Ed25519 PureEdDSA over the 32-byte SHA-256 digest.
#[must_use]
pub fn sign(preimage: &[u8; 32], key: &SigningKey) -> [u8; 64] {
    let sig: Signature = key.sign(preimage);
    sig.to_bytes()
}

/// Verify `sig` against any key in `keys`.
pub fn verify_signature(
    preimage: &[u8; 32],
    sig: &[u8; 64],
    keys: &[VerifyingKey],
) -> Result<(), PackageError> {
    if keys.is_empty() {
        return Err(PackageError::Signature);
    }
    let signature = Signature::from_bytes(sig);
    for key in keys {
        if key.verify(preimage, &signature).is_ok() {
            return Ok(());
        }
    }
    Err(PackageError::Signature)
}

/// Apply [`VerifyPolicy`] to a parsed package's signature and preimage.
pub fn check_policy(
    policy: VerifyPolicy<'_>,
    signature: &[u8; 64],
    preimage: &[u8; 32],
) -> Result<(), PackageError> {
    if !policy.require_signature {
        return Ok(());
    }
    if *signature == [0u8; 64] {
        return Err(PackageError::Unsigned);
    }
    verify_signature(preimage, signature, policy.public_keys)
}

/// Build a signing key from a 32-byte seed (tests / offline packager).
#[must_use]
pub fn signing_key_from_seed(seed: &[u8; 32]) -> SigningKey {
    SigningKey::from_bytes(seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify_round_trip() {
        let seed = [7u8; 32];
        let sk = signing_key_from_seed(&seed);
        let vk = sk.verifying_key();
        let pre = signing_preimage(b"{\"a\":1}", &[b"SPBC".as_slice()]);
        let sig = sign(&pre, &sk);
        verify_signature(&pre, &sig, &[vk]).unwrap();
        let mut bad = sig;
        bad[0] ^= 1;
        assert_eq!(
            verify_signature(&pre, &bad, &[vk]).unwrap_err(),
            PackageError::Signature
        );
    }
}
