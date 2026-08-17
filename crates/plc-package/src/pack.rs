//! Build a signed or unsigned `.spkg` v1.

use ed25519_dalek::SigningKey;
use plc_ir::{write_spbc, IrModule};

use crate::compat::compute_compatibility_hash;
use crate::error::PackageError;
use crate::format::{write_spkg, SIGNATURE_LEN};
use crate::manifest::Manifest;
use crate::sign::{sign, signing_preimage};

enum SigMode {
    Pending,
    Key(Box<SigningKey>),
    Unsigned,
}

/// Assemble one closed `spbc` image into a package major 1 container.
pub struct PackageBuilder {
    manifest: Manifest,
    sections: Vec<Vec<u8>>,
    sig: SigMode,
}

impl PackageBuilder {
    /// Start a builder. `compatibility_hash` is recomputed on [`Self::to_bytes`].
    #[must_use]
    pub fn new(manifest: Manifest) -> Self {
        Self {
            manifest,
            sections: Vec::new(),
            sig: SigMode::Pending,
        }
    }

    /// Append a raw `spbc` blob. v1 builders accept exactly one section.
    pub fn section_bytes(mut self, spbc: Vec<u8>) -> Result<Self, PackageError> {
        if !self.sections.is_empty() {
            return Err(PackageError::SectionCount(2));
        }
        self.sections.push(spbc);
        Ok(self)
    }

    /// Encode `module` as `spbc` and append it.
    pub fn section_module(self, module: &IrModule) -> Result<Self, PackageError> {
        self.section_bytes(write_spbc(module)?)
    }

    /// Sign with `key` (non-zero Ed25519 signature).
    #[must_use]
    pub fn sign(mut self, key: &SigningKey) -> Self {
        self.sig = SigMode::Key(Box::new(key.clone()));
        self
    }

    /// Write the all-zero signature sentinel (dev / `require_signature=false`).
    #[must_use]
    pub fn unsigned(mut self) -> Self {
        self.sig = SigMode::Unsigned;
        self
    }

    /// Compute hash, JCS-serialize the manifest, sign, and frame.
    pub fn to_bytes(mut self) -> Result<Vec<u8>, PackageError> {
        if self.sections.len() != 1 {
            return Err(PackageError::SectionCount(self.sections.len() as u32));
        }
        self.manifest.compatibility_hash = compute_compatibility_hash(&self.manifest);
        self.manifest.validate_fields()?;
        let canonical = self.manifest.to_jcs_bytes()?;
        let preimage = signing_preimage(&canonical, &self.sections);
        let signature: [u8; SIGNATURE_LEN] = match &self.sig {
            SigMode::Pending => {
                return Err(PackageError::manifest(
                    "call sign() or unsigned() before to_bytes()",
                ));
            }
            SigMode::Key(key) => sign(&preimage, key),
            SigMode::Unsigned => [0u8; SIGNATURE_LEN],
        };
        write_spkg(&canonical, &self.sections, &signature)
    }
}
