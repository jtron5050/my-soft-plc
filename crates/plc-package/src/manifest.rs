//! Package manifest types (JSON, package major 1).

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use plc_ir::{IrType, RetainLayout, RetainSymbol};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::PackageError;
use crate::jcs::parse_strict_json;

/// Output restart policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestartPolicy {
    /// Force `safe_state` for one logic pass, then the program drives `%Q`.
    SafeReset,
    /// Hold last `%Q` through the first post-activate invocation when eligible.
    Bumpless,
}

/// Process-image / dictionary region for a tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum TagKind {
    /// `%I` input.
    I,
    /// `%Q` output (feeds `compatibility_hash`).
    Q,
    /// `%M` volatile.
    M,
    /// Retain / `%R`.
    R,
    /// Internal / system tag (not an image slot).
    Internal,
}

/// IEC type name as used in the JSON manifest (`BOOL`, `DINT`, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IrTypeName(pub IrType);

impl IrTypeName {
    /// Uppercase IEC name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self.0 {
            IrType::Bool => "BOOL",
            IrType::Int => "INT",
            IrType::Dint => "DINT",
            IrType::Real => "REAL",
            IrType::Time => "TIME",
            IrType::Lint => "LINT",
        }
    }
}

impl From<IrType> for IrTypeName {
    fn from(ty: IrType) -> Self {
        Self(ty)
    }
}

impl From<IrTypeName> for IrType {
    fn from(name: IrTypeName) -> Self {
        name.0
    }
}

impl fmt::Display for IrTypeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for IrTypeName {
    type Err = PackageError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let ty = match s {
            "BOOL" => IrType::Bool,
            "INT" => IrType::Int,
            "DINT" => IrType::Dint,
            "REAL" => IrType::Real,
            "TIME" => IrType::Time,
            "LINT" => IrType::Lint,
            other => {
                return Err(PackageError::manifest(format!(
                    "unknown IR type {other:?} (use BOOL/INT/DINT/REAL/TIME/LINT)"
                )));
            }
        };
        Ok(Self(ty))
    }
}

impl Serialize for IrTypeName {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for IrTypeName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// One retained symbol in the manifest (name + type + retain-segment offset).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestRetainSymbol {
    /// IEC path (e.g. `Line.Hours`).
    pub name: String,
    /// Value type.
    #[serde(rename = "type")]
    pub ty: IrTypeName,
    /// Byte offset in the retain segment.
    pub offset: u32,
}

/// One tag-dictionary entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TagEntry {
    /// Tag path used by telemetry / tools.
    pub name: String,
    /// Value type.
    #[serde(rename = "type")]
    pub ty: IrTypeName,
    /// Image region.
    pub kind: TagKind,
    /// Optional typed slot index for `%I` / `%Q`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u32>,
}

/// `.spkg` v1 JSON manifest (normative keys only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Program id (retain store path segment).
    pub id: String,
    /// Semver string (advisory; compatibility is the hash).
    pub version: String,
    /// Opaque build identifier.
    pub build_id: String,
    /// Must match `spbc` / [`plc_ir::IR_MAJOR`].
    pub ir_major: u16,
    /// Must match `spbc` / [`plc_ir::IR_MINOR`].
    pub ir_minor: u16,
    /// Primitive ABI number; must match the runtime that will execute the package.
    pub primitive_abi: u32,
    /// Task name → IR entry symbol (e.g. `main` → `task.main`).
    pub task_entries: BTreeMap<String, String>,
    /// Retain symbols; may be empty. Offsets are required for `RetainLayout`.
    pub retain_symbols: Vec<ManifestRetainSymbol>,
    /// Tag dictionary; `%Q` entries feed `compatibility_hash`.
    pub tag_dictionary: Vec<TagEntry>,
    /// Output restart policy.
    pub restart_policy: RestartPolicy,
    /// SHA-256 hex (lowercase) of the compatibility preimage.
    pub compatibility_hash: String,
    /// Optional; must match `spbc` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_slots: Option<u32>,
    /// Optional; must match `spbc` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_slots: Option<u32>,
    /// Optional; must match `spbc` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_size: Option<u32>,
    /// Optional; must match `spbc` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retain_size: Option<u32>,
    /// Optional; must match `spbc` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub const_size: Option<u32>,
}

impl Manifest {
    /// Parse UTF-8 JSON into a typed manifest after the strict/JCS gate.
    pub fn from_json_bytes(bytes: &[u8]) -> Result<(Self, Vec<u8>), PackageError> {
        let strict = parse_strict_json(bytes)?;
        let canonical = strict.to_jcs()?;
        let mut de = serde_json::Deserializer::from_slice(bytes);
        let manifest: Self =
            Deserialize::deserialize(&mut de).map_err(|e| PackageError::json(e.to_string()))?;
        de.end()
            .map_err(|e| PackageError::json(format!("trailing data after JSON: {e}")))?;
        manifest.validate_fields()?;
        Ok((manifest, canonical))
    }

    /// JCS bytes of this typed manifest (builder path).
    pub fn to_jcs_bytes(&self) -> Result<Vec<u8>, PackageError> {
        let compact = serde_json::to_vec(self).map_err(|e| PackageError::jcs(e.to_string()))?;
        let strict = parse_strict_json(&compact)?;
        strict.to_jcs()
    }

    /// Field-level checks that serde does not cover.
    pub fn validate_fields(&self) -> Result<(), PackageError> {
        validate_program_id(&self.id)?;
        semver::Version::parse(&self.version).map_err(|e| {
            PackageError::manifest(format!("version {:?} is not semver: {e}", self.version))
        })?;
        if self.build_id.is_empty() {
            return Err(PackageError::manifest("build_id must be non-empty"));
        }
        if self.task_entries.is_empty() {
            return Err(PackageError::manifest("task_entries must not be empty"));
        }
        for (name, symbol) in &self.task_entries {
            if name.is_empty() || symbol.is_empty() {
                return Err(PackageError::manifest(
                    "task_entries keys and values must be non-empty",
                ));
            }
            reject_ascii_controls("task name", name)?;
        }
        for sym in &self.retain_symbols {
            reject_ascii_controls("retain name", &sym.name)?;
        }
        for tag in &self.tag_dictionary {
            if tag.kind == TagKind::Q {
                reject_ascii_controls("%Q name", &tag.name)?;
            }
        }
        if !is_lowercase_hex_sha256(&self.compatibility_hash) {
            return Err(PackageError::manifest(
                "compatibility_hash must be 64 lowercase hex characters",
            ));
        }
        Ok(())
    }

    /// Build a [`RetainLayout`] from `retain_symbols` and the authoritative size.
    pub fn retain_layout(&self, retain_size: u32) -> Result<RetainLayout, PackageError> {
        let symbols = self
            .retain_symbols
            .iter()
            .map(|s| RetainSymbol::new(s.name.clone(), s.ty.0, s.offset))
            .collect();
        RetainLayout::new(retain_size, symbols).map_err(|e| match e {
            plc_ir::IrError::RetainLayout(msg) => PackageError::mismatch(msg),
            other => PackageError::from(other),
        })
    }
}

/// Same charset as `plc-retain::validate_program_id` (kept independent of that crate).
pub fn validate_program_id(id: &str) -> Result<(), PackageError> {
    if id.is_empty()
        || id == "."
        || id == ".."
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
    {
        return Err(PackageError::InvalidProgramId(id.to_string()));
    }
    Ok(())
}

fn is_lowercase_hex_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Compatibility preimage uses `\t` / `\n` as delimiters with no escaping.
fn reject_ascii_controls(label: &str, name: &str) -> Result<(), PackageError> {
    if name.bytes().any(|b| b < 0x20 || b == 0x7F) {
        return Err(PackageError::manifest(format!(
            "{label} must not contain ASCII control characters"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_field_and_bumpless_bool() {
        let json = br#"{
            "id": "plant-line-a",
            "version": "1.0.0",
            "build_id": "b",
            "ir_major": 0,
            "ir_minor": 1,
            "primitive_abi": 1,
            "task_entries": {"main": "task.main"},
            "retain_symbols": [],
            "tag_dictionary": [],
            "restart_policy": "safe_reset",
            "compatibility_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "bumpless": true
        }"#;
        let err = Manifest::from_json_bytes(json).unwrap_err();
        assert!(err.to_string().contains("unknown field") || err.to_string().contains("bumpless"));
    }

    #[test]
    fn rejects_bad_program_id() {
        assert!(validate_program_id("../x").is_err());
        assert!(validate_program_id("ok_id.1").is_ok());
    }

    #[test]
    fn rejects_unknown_nested_field_on_retain_symbol() {
        let json = br#"{
            "id": "plant-line-a",
            "version": "1.0.0",
            "build_id": "b",
            "ir_major": 0,
            "ir_minor": 1,
            "primitive_abi": 1,
            "task_entries": {"main": "task.main"},
            "retain_symbols": [{"name": "Line.Hours", "type": "DINT", "offset": 0, "extra": 1}],
            "tag_dictionary": [],
            "restart_policy": "safe_reset",
            "compatibility_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        }"#;
        let err = Manifest::from_json_bytes(json).unwrap_err();
        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn rejects_tab_in_retain_name() {
        let json = br#"{
            "id": "plant-line-a",
            "version": "1.0.0",
            "build_id": "b",
            "ir_major": 0,
            "ir_minor": 1,
            "primitive_abi": 1,
            "task_entries": {"main": "task.main"},
            "retain_symbols": [{"name": "Line\tHours", "type": "DINT", "offset": 0}],
            "tag_dictionary": [],
            "restart_policy": "safe_reset",
            "compatibility_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        }"#;
        let err = Manifest::from_json_bytes(json).unwrap_err();
        assert!(
            err.to_string().contains("retain name") && err.to_string().contains("control"),
            "{err}"
        );
    }
}
