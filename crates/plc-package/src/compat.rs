//! `compatibility_hash` preimage (compiler and runtime must match).

use sha2::{Digest, Sha256};

use crate::manifest::{Manifest, TagKind};

/// SHA-256 hex (lowercase, no `0x`) of the compatibility preimage.
///
/// Preimage (UTF-8):
/// ```text
/// ir_major=<decimal>\n
/// primitive_abi=<decimal>\n
/// R\t<name>\t<TYPE>\n          # retain_symbols sorted by name
/// Q\t<name>\t<TYPE>\n          # tag_dictionary kind=Q sorted by name
/// T\t<task-name>\n             # task_entries keys sorted by name
/// ```
#[must_use]
pub fn compute_compatibility_hash(manifest: &Manifest) -> String {
    hex_encode(&Sha256::digest(compatibility_preimage(manifest)))
}

/// Raw compatibility preimage bytes.
#[must_use]
pub fn compatibility_preimage(manifest: &Manifest) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(format!("ir_major={}\n", manifest.ir_major).as_bytes());
    out.extend_from_slice(format!("primitive_abi={}\n", manifest.primitive_abi).as_bytes());

    let mut retain: Vec<_> = manifest.retain_symbols.iter().collect();
    retain.sort_by(|a, b| a.name.cmp(&b.name));
    for sym in retain {
        out.extend_from_slice(b"R\t");
        out.extend_from_slice(sym.name.as_bytes());
        out.push(b'\t');
        out.extend_from_slice(sym.ty.as_str().as_bytes());
        out.push(b'\n');
    }

    let mut q_tags: Vec<_> = manifest
        .tag_dictionary
        .iter()
        .filter(|t| t.kind == TagKind::Q)
        .collect();
    q_tags.sort_by(|a, b| a.name.cmp(&b.name));
    for tag in q_tags {
        out.extend_from_slice(b"Q\t");
        out.extend_from_slice(tag.name.as_bytes());
        out.push(b'\t');
        out.extend_from_slice(tag.ty.as_str().as_bytes());
        out.push(b'\n');
    }

    let mut tasks: Vec<_> = manifest.task_entries.keys().collect();
    tasks.sort();
    for name in tasks {
        out.extend_from_slice(b"T\t");
        out.extend_from_slice(name.as_bytes());
        out.push(b'\n');
    }
    out
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

/// Decode a hex string (lowercase or uppercase) into `N` bytes.
pub fn hex_decode_n<const N: usize>(s: &str) -> Result<[u8; N], crate::error::PackageError> {
    let t = s.trim();
    if t.len() != N * 2 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(crate::error::PackageError::manifest(format!(
            "expected {N} hex bytes"
        )));
    }
    let mut out = [0u8; N];
    for i in 0..N {
        out[i] = u8::from_str_radix(&t[i * 2..i * 2 + 2], 16)
            .map_err(|_| crate::error::PackageError::manifest("invalid hex digit"))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{IrTypeName, Manifest, ManifestRetainSymbol, RestartPolicy, TagEntry};
    use plc_ir::IrType;
    use std::collections::BTreeMap;

    fn base_manifest() -> Manifest {
        let mut task_entries = BTreeMap::new();
        task_entries.insert("main".into(), "task.main".into());
        Manifest {
            id: "plant-line-a".into(),
            version: "1.0.0".into(),
            build_id: "b1".into(),
            ir_major: 0,
            ir_minor: 1,
            primitive_abi: 1,
            task_entries,
            retain_symbols: vec![
                ManifestRetainSymbol {
                    name: "b".into(),
                    ty: IrTypeName(IrType::Int),
                    offset: 2,
                },
                ManifestRetainSymbol {
                    name: "a".into(),
                    ty: IrTypeName(IrType::Bool),
                    offset: 0,
                },
            ],
            tag_dictionary: vec![
                TagEntry {
                    name: "Q.B".into(),
                    ty: IrTypeName(IrType::Bool),
                    kind: TagKind::Q,
                    slot: Some(1),
                },
                TagEntry {
                    name: "Q.A".into(),
                    ty: IrTypeName(IrType::Bool),
                    kind: TagKind::Q,
                    slot: Some(0),
                },
                TagEntry {
                    name: "I.X".into(),
                    ty: IrTypeName(IrType::Bool),
                    kind: TagKind::I,
                    slot: Some(0),
                },
            ],
            restart_policy: RestartPolicy::SafeReset,
            compatibility_hash: "00".repeat(32),
            input_slots: None,
            output_slots: None,
            data_size: None,
            retain_size: None,
            const_size: None,
        }
    }

    #[test]
    fn hash_is_order_independent_and_ignores_offsets() {
        let a = base_manifest();
        let mut b = base_manifest();
        b.retain_symbols.reverse();
        b.tag_dictionary.reverse();
        b.retain_symbols[0].offset = 99;
        assert_eq!(
            compute_compatibility_hash(&a),
            compute_compatibility_hash(&b)
        );
        let mut c = base_manifest();
        c.ir_major = 1;
        assert_ne!(
            compute_compatibility_hash(&a),
            compute_compatibility_hash(&c)
        );
    }
}
