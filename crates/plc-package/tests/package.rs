//! Package format, JCS, signature, and validate coverage (PR-09).

use std::collections::BTreeMap;

use plc_ir::{assemble, write_spbc, IrType};
use plc_package::{
    compute_compatibility_hash, hex_decode_n, hex_encode, parse_spkg, sign, signing_key_from_seed,
    signing_preimage, validate, write_spkg, IrTypeName, Manifest, ManifestRetainSymbol,
    PackageBuilder, PackageError, RestartPolicy, TagEntry, TagKind, VerifyPolicy,
    MAX_PACKAGE_BYTES, SIGNATURE_LEN, SPKG_MAGIC,
};

const SEED_HEX: &str = include_str!("keys/test-ed25519.seed.hex");
const PUB_HEX: &str = include_str!("keys/test-ed25519.pub.hex");
const GOLDEN_SPKG: &[u8] = include_bytes!("fixtures/minimal.spkg");
const TON_CALL: &str = include_str!("../../../samples/programs/ton-call/fixture.spasm");

fn test_seed() -> [u8; 32] {
    hex_decode_n(SEED_HEX).expect("seed hex")
}

fn test_signing_key() -> plc_package::SigningKey {
    signing_key_from_seed(&test_seed())
}

fn test_verifying_key() -> plc_package::VerifyingKey {
    test_signing_key().verifying_key()
}

fn halt_module() -> plc_ir::IrModule {
    assemble(
        r#"
.header data_size=32 retain_size=0 input_slots=0 output_slots=0
.entry task.main
HALT
"#,
    )
    .unwrap()
}

fn placeholder_hash() -> String {
    "00".repeat(32)
}

fn manifest_for(module: &plc_ir::IrModule) -> Manifest {
    let mut task_entries = BTreeMap::new();
    let symbol = module
        .entries
        .first()
        .map_or_else(|| "task.main".into(), |e| e.name.clone());
    task_entries.insert("main".into(), symbol);
    let mut m = Manifest {
        id: "minimal".into(),
        version: "0.1.0".into(),
        build_id: "golden-1".into(),
        ir_major: module.ir_major,
        ir_minor: module.ir_minor,
        primitive_abi: 1,
        task_entries,
        retain_symbols: Vec::new(),
        tag_dictionary: Vec::new(),
        restart_policy: RestartPolicy::SafeReset,
        compatibility_hash: placeholder_hash(),
        input_slots: Some(module.input_slots),
        output_slots: Some(module.output_slots),
        data_size: Some(module.data_size),
        retain_size: Some(module.retain_size),
        const_size: Some(module.const_size),
    };
    m.compatibility_hash = compute_compatibility_hash(&m);
    m
}

fn signed_bytes(manifest: Manifest, module: &plc_ir::IrModule) -> Vec<u8> {
    PackageBuilder::new(manifest)
        .section_module(module)
        .unwrap()
        .sign(&test_signing_key())
        .to_bytes()
        .unwrap()
}

#[test]
fn test_public_key_matches_checked_in_hex() {
    let vk = test_verifying_key();
    assert_eq!(hex_encode(vk.as_bytes()), PUB_HEX.trim());
}

#[test]
fn golden_minimal_spkg_matches_builder() {
    let module = halt_module();
    let bytes = signed_bytes(manifest_for(&module), &module);
    assert_eq!(bytes, GOLDEN_SPKG, "rebuild tests/fixtures/minimal.spkg");
    let parsed = validate(&bytes, VerifyPolicy::required(&[test_verifying_key()])).unwrap();
    assert_eq!(parsed.manifest.id, "minimal");
    assert_eq!(parsed.sections.len(), 1);
}

#[test]
fn pack_parse_round_trip() {
    let module = halt_module();
    let bytes = signed_bytes(manifest_for(&module), &module);
    let framed = parse_spkg(&bytes).unwrap();
    assert_eq!(framed.manifest.id, "minimal");
    assert!(
        framed.modules.is_empty(),
        "parse_spkg must not run parse_spbc"
    );
    assert_eq!(write_spbc(&module).unwrap(), framed.sections[0]);
    let parsed = validate(&bytes, VerifyPolicy::required(&[test_verifying_key()])).unwrap();
    assert_eq!(parsed.modules[0].code, module.code);
    assert_eq!(write_spbc(&parsed.modules[0]).unwrap(), parsed.sections[0]);
}

#[test]
fn ton_call_fixture_packs_and_validates() {
    let module = assemble(TON_CALL).expect("assemble ton-call");
    let bytes = signed_bytes(manifest_for(&module), &module);
    validate(&bytes, VerifyPolicy::required(&[test_verifying_key()])).unwrap();
}

#[test]
fn pretty_and_compact_json_share_jcs_and_verify() {
    let module = halt_module();
    let manifest = manifest_for(&module);
    let section = write_spbc(&module).unwrap();
    let compact = manifest.to_jcs_bytes().unwrap();
    let pretty = serde_json::to_vec_pretty(&manifest).unwrap();
    assert_ne!(pretty, compact);
    let pre = signing_preimage(&compact, std::slice::from_ref(&section));
    let sig = sign(&pre, &test_signing_key());
    let pretty_pkg = write_spkg(&pretty, &[section.clone()], &sig).unwrap();
    let compact_pkg = write_spkg(&compact, &[section], &sig).unwrap();
    let a = validate(&pretty_pkg, VerifyPolicy::required(&[test_verifying_key()])).unwrap();
    let b = validate(
        &compact_pkg,
        VerifyPolicy::required(&[test_verifying_key()]),
    )
    .unwrap();
    assert_eq!(a.manifest_canonical, b.manifest_canonical);
}

#[test]
fn tampered_manifest_fails_signature() {
    let module = halt_module();
    let mut bytes = signed_bytes(manifest_for(&module), &module);
    // Flip a byte inside the stored JSON (after 10-byte header).
    bytes[12] ^= 0x01;
    let err = validate(&bytes, VerifyPolicy::required(&[test_verifying_key()])).unwrap_err();
    assert!(
        matches!(
            err,
            PackageError::Signature | PackageError::Json(_) | PackageError::Manifest(_)
        ),
        "{err:?}"
    );
}

#[test]
fn tampered_bytecode_fails_signature() {
    let module = halt_module();
    let mut bytes = signed_bytes(manifest_for(&module), &module);
    // Last 64 bytes are the signature; flip a payload byte just before that.
    let i = bytes.len() - SIGNATURE_LEN - 1;
    bytes[i] ^= 0x01;
    assert_eq!(
        validate(&bytes, VerifyPolicy::required(&[test_verifying_key()])).unwrap_err(),
        PackageError::Signature
    );
}

#[test]
fn wrong_verifying_key_fails() {
    let module = halt_module();
    let bytes = signed_bytes(manifest_for(&module), &module);
    let other = signing_key_from_seed(&[9u8; 32]).verifying_key();
    assert_eq!(
        validate(&bytes, VerifyPolicy::required(&[other])).unwrap_err(),
        PackageError::Signature
    );
}

#[test]
fn unsigned_sentinel_policy() {
    let module = halt_module();
    let bytes = PackageBuilder::new(manifest_for(&module))
        .section_module(&module)
        .unwrap()
        .unsigned()
        .to_bytes()
        .unwrap();
    validate(&bytes, VerifyPolicy::unsigned()).unwrap();
    assert_eq!(
        validate(&bytes, VerifyPolicy::required(&[test_verifying_key()])).unwrap_err(),
        PackageError::Unsigned
    );
}

#[test]
fn oversized_rejected() {
    let n = MAX_PACKAGE_BYTES + 1;
    assert_eq!(
        validate(&vec![0u8; n], VerifyPolicy::unsigned()).unwrap_err(),
        PackageError::TooLarge(n)
    );
}

#[test]
fn unsupported_version() {
    let mut bytes = Vec::from(*SPKG_MAGIC);
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0u8; SIGNATURE_LEN]);
    assert_eq!(
        parse_spkg(&bytes).unwrap_err(),
        PackageError::UnsupportedVersion(2)
    );
}

#[test]
fn cbor_file_rejected() {
    assert_eq!(
        parse_spkg(&[0xD9, 0xD9, 0xF7, 0xA1, 0x01, 0x02]).unwrap_err(),
        PackageError::CborRejected
    );
}

#[test]
fn duplicate_json_key_rejected() {
    let module = halt_module();
    let section = write_spbc(&module).unwrap();
    let json = br#"{"build_id":"b","compatibility_hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","id":"minimal","id":"other","ir_major":0,"ir_minor":1,"primitive_abi":1,"restart_policy":"safe_reset","retain_symbols":[],"tag_dictionary":[],"task_entries":{"main":"task.main"},"version":"0.1.0"}"#;
    let pkg = write_spkg(json, &[section], &[0u8; SIGNATURE_LEN]).unwrap();
    let err = parse_spkg(&pkg).unwrap_err();
    assert!(err.to_string().contains("duplicate key"), "{err}");
}

#[test]
fn json_comment_rejected() {
    let module = halt_module();
    let section = write_spbc(&module).unwrap();
    let json = br#"{"id":"minimal" /* nope */}"#;
    let pkg = write_spkg(json, &[section], &[0u8; SIGNATURE_LEN]).unwrap();
    assert!(matches!(
        parse_spkg(&pkg).unwrap_err(),
        PackageError::Json(_)
    ));
}

#[test]
fn non_utf8_manifest_rejected() {
    let module = halt_module();
    let section = write_spbc(&module).unwrap();
    let pkg = write_spkg(&[0xFF, 0xFE, 0xFD], &[section], &[0u8; SIGNATURE_LEN]).unwrap();
    let err = parse_spkg(&pkg).unwrap_err();
    assert!(
        matches!(err, PackageError::Json(_) | PackageError::CborRejected),
        "{err:?}"
    );
}

#[test]
fn input_slots_mismatch() {
    let module = halt_module();
    let mut manifest = manifest_for(&module);
    manifest.input_slots = Some(99);
    let bytes = PackageBuilder::new(manifest)
        .section_module(&module)
        .unwrap()
        .unsigned()
        .to_bytes()
        .unwrap();
    assert!(matches!(
        validate(&bytes, VerifyPolicy::unsigned()).unwrap_err(),
        PackageError::ManifestSpbcMismatch(_)
    ));
}

#[test]
fn missing_task_entry_symbol() {
    let module = halt_module();
    let mut manifest = manifest_for(&module);
    manifest.task_entries.clear();
    manifest
        .task_entries
        .insert("main".into(), "task.nope".into());
    manifest.compatibility_hash = compute_compatibility_hash(&manifest);
    let bytes = PackageBuilder::new(manifest)
        .section_module(&module)
        .unwrap()
        .unsigned()
        .to_bytes()
        .unwrap();
    assert!(matches!(
        validate(&bytes, VerifyPolicy::unsigned()).unwrap_err(),
        PackageError::ManifestSpbcMismatch(_)
    ));
}

#[test]
fn compatibility_hash_mismatch() {
    let module = halt_module();
    let mut manifest = manifest_for(&module);
    manifest.compatibility_hash = "ab".repeat(32);
    // Bypass builder hash rewrite by framing by hand.
    let section = write_spbc(&module).unwrap();
    let json = serde_json::to_vec(&manifest).unwrap();
    let pkg = write_spkg(&json, &[section], &[0u8; SIGNATURE_LEN]).unwrap();
    assert_eq!(
        validate(&pkg, VerifyPolicy::unsigned()).unwrap_err(),
        PackageError::CompatibilityHash
    );
}

#[test]
fn retain_oob_rejected() {
    let module = assemble(
        r#"
.header data_size=32 retain_size=2 input_slots=0 output_slots=0
.entry task.main
HALT
"#,
    )
    .unwrap();
    let mut manifest = manifest_for(&module);
    manifest.retain_symbols.push(ManifestRetainSymbol {
        name: "Line.Hours".into(),
        ty: IrTypeName(IrType::Dint),
        offset: 0,
    });
    manifest.compatibility_hash = compute_compatibility_hash(&manifest);
    let bytes = PackageBuilder::new(manifest)
        .section_module(&module)
        .unwrap()
        .unsigned()
        .to_bytes()
        .unwrap();
    let err = validate(&bytes, VerifyPolicy::unsigned()).unwrap_err();
    assert!(
        matches!(err, PackageError::ManifestSpbcMismatch(_)),
        "{err}"
    );
}

#[test]
fn ir_verify_failure() {
    let module = assemble(
        r#"
.header data_size=32 retain_size=0 input_slots=0 output_slots=0
.entry task.main
NOP
"#,
    )
    .unwrap();
    let bytes = PackageBuilder::new(manifest_for(&module))
        .section_module(&module)
        .unwrap()
        .unsigned()
        .to_bytes()
        .unwrap();
    assert!(matches!(
        validate(&bytes, VerifyPolicy::unsigned()).unwrap_err(),
        PackageError::Verify(_)
    ));
}

#[test]
fn policy_checked_before_spbc_parse() {
    let module = halt_module();
    let manifest = manifest_for(&module);
    let json = manifest.to_jcs_bytes().unwrap();
    let pkg = write_spkg(&json, &[b"not-spbc".to_vec()], &[0u8; SIGNATURE_LEN]).unwrap();
    assert!(
        parse_spkg(&pkg).is_ok(),
        "framing must succeed without parsing the section"
    );
    assert_eq!(
        validate(&pkg, VerifyPolicy::required(&[test_verifying_key()])).unwrap_err(),
        PackageError::Unsigned
    );
}

#[test]
fn two_sections_parse_but_validate_rejects() {
    let module = halt_module();
    let section = write_spbc(&module).unwrap();
    let manifest = manifest_for(&module);
    let json = manifest.to_jcs_bytes().unwrap();
    let pkg = write_spkg(&json, &[section.clone(), section], &[0u8; SIGNATURE_LEN]).unwrap();
    let parsed = parse_spkg(&pkg).unwrap();
    assert_eq!(parsed.sections.len(), 2);
    assert_eq!(
        validate(&pkg, VerifyPolicy::unsigned()).unwrap_err(),
        PackageError::SectionCount(2)
    );
}

#[test]
fn trailing_garbage_rejected() {
    let module = halt_module();
    let mut bytes = signed_bytes(manifest_for(&module), &module);
    bytes.push(0);
    assert_eq!(parse_spkg(&bytes).unwrap_err(), PackageError::TrailingBytes);
}

#[test]
fn tag_q_feeds_compatibility_hash() {
    let module = halt_module();
    let mut a = manifest_for(&module);
    a.tag_dictionary.push(TagEntry {
        name: "Line.RunFwd".into(),
        ty: IrTypeName(IrType::Bool),
        kind: TagKind::Q,
        slot: Some(0),
    });
    let mut b = a.clone();
    assert_ne!(
        compute_compatibility_hash(&a),
        compute_compatibility_hash(&manifest_for(&module))
    );
    b.tag_dictionary[0].slot = Some(9);
    assert_eq!(
        compute_compatibility_hash(&a),
        compute_compatibility_hash(&b)
    );
}
