//! Golden vectors from architecture Appendix A.8 / A.9.

use plc_ir::{
    assemble, decode_instruction, encode_instruction, pack_word, verify_module, write_spbc,
    DecodedInstr, Opcode, PrimitiveId,
};

#[test]
fn rs_latch_opcode_words_match_appendix() {
    let src = include_str!("fixtures/rs_latch.spasm");
    let m = assemble(src).expect("assemble rs_latch");
    verify_module(&m).expect("verify rs_latch");

    // Expected LE u32 words from Appendix A.8
    let expected: &[u32] = &[
        0x1000_0000, // LD_DATA 0
        0x1000_0002, // LD_DATA 2
        0x2900_0000, // OR
        0x1000_0001, // LD_DATA 1
        0x2B00_0000, // NOT
        0x2800_0000, // AND
        0x1100_0002, // ST_DATA 2
        0x5100_0000, // RET
    ];
    assert_eq!(m.code.len(), expected.len() * 4);
    for (i, exp) in expected.iter().enumerate() {
        let word = u32::from_le_bytes(m.code[i * 4..i * 4 + 4].try_into().unwrap());
        assert_eq!(word, *exp, "word {i}");
    }

    assert!(m.entries[0].is_user_fb);
    assert_eq!(m.entries[0].name, "fb.RS");
}

#[test]
fn ton_call_assembles_and_verifies() {
    let src = include_str!("fixtures/ton_call.spasm");
    let m = assemble(src).expect("assemble ton_call");
    verify_module(&m).expect("verify ton_call");

    // Walk instructions: PUSHI_BOOL, PUSH_TIME, CALL_FB, ST_DATA, ST_DATA, HALT
    let mut pc = 0usize;
    let (i0, n0) = decode_instruction(&m.code, pc).unwrap();
    assert!(matches!(
        i0,
        DecodedInstr::Simple {
            op: Opcode::PushIBool,
            payload: 1
        }
    ));
    pc += n0;

    let (i1, n1) = decode_instruction(&m.code, pc).unwrap();
    match i1 {
        DecodedInstr::WithImm32 {
            op: Opcode::PushTime,
            imm,
            ..
        } => assert_eq!(imm, 1000),
        other => panic!("expected PUSH_TIME, got {other:?}"),
    }
    pc += n1;

    let (i2, n2) = decode_instruction(&m.code, pc).unwrap();
    match i2 {
        DecodedInstr::CallFb {
            fb_kind,
            fb_id,
            instance_base,
        } => {
            assert_eq!(fb_kind, 0);
            assert_eq!(fb_id, PrimitiveId::Ton as u32);
            assert_eq!(instance_base, 0x40);
        }
        other => panic!("expected CALL_FB, got {other:?}"),
    }
    pc += n2;

    let (i3, n3) = decode_instruction(&m.code, pc).unwrap();
    assert!(matches!(
        i3,
        DecodedInstr::Simple {
            op: Opcode::StData,
            payload: 0
        }
    ));
    pc += n3;

    let (i4, n4) = decode_instruction(&m.code, pc).unwrap();
    assert!(matches!(
        i4,
        DecodedInstr::Simple {
            op: Opcode::StData,
            payload: 4
        }
    ));
    pc += n4;

    let (i5, _) = decode_instruction(&m.code, pc).unwrap();
    assert!(matches!(
        i5,
        DecodedInstr::Simple {
            op: Opcode::Halt,
            ..
        }
    ));
}

#[test]
fn pack_word_helpers_match_appendix_schematic() {
    assert_eq!(pack_word(Opcode::LdData, 0), 0x1000_0000);
    assert_eq!(pack_word(Opcode::Or, 0), 0x2900_0000);
    assert_eq!(pack_word(Opcode::Ret, 0), 0x5100_0000);
}

#[test]
fn spbc_round_trip_rs() {
    let src = include_str!("fixtures/rs_latch.spasm");
    let m = assemble(src).unwrap();
    let bytes = write_spbc(&m).unwrap();
    assert_eq!(&bytes[0..4], b"SPBC");
    let m2 = plc_ir::parse_spbc(&bytes).unwrap();
    assert_eq!(m2.code, m.code);
    verify_module(&m2).unwrap();
}

#[test]
fn encode_decode_call_fb() {
    let instr = DecodedInstr::CallFb {
        fb_kind: 0,
        fb_id: PrimitiveId::Ton as u32,
        instance_base: 0x40,
    };
    let bytes = encode_instruction(&instr);
    let (decoded, n) = decode_instruction(&bytes, 0).unwrap();
    assert_eq!(n, 12);
    assert_eq!(decoded, instr);
}
