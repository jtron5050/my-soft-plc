//! Encode / decode 32-bit instruction words.

use crate::error::IrError;
use crate::opcode::Opcode;

/// Decoded instruction (one or more words).
#[derive(Debug, Clone, PartialEq)]
pub enum DecodedInstr {
    /// Simple opcode with u24 payload (often 0).
    Simple {
        /// Opcode.
        op: Opcode,
        /// Lower 24 bits.
        payload: u32,
    },
    /// Opcode + following i32/f32 word.
    WithImm32 {
        /// Opcode.
        op: Opcode,
        /// Payload in first word.
        payload: u32,
        /// Immediate word (raw bits).
        imm: u32,
    },
    /// CALL_FB: opcode word + fb_kind/id word + instance base word.
    CallFb {
        /// 0 = primitive, 1 = user.
        fb_kind: u32,
        /// Primitive or user FB id.
        fb_id: u32,
        /// Instance base offset in data/retain.
        instance_base: u32,
    },
}

/// Pack opcode + 24-bit payload into LE u32.
#[must_use]
pub const fn pack_word(op: Opcode, payload: u32) -> u32 {
    ((op as u8 as u32) << 24) | (payload & 0x00FF_FFFF)
}

/// Encode a decoded instruction to bytes (LE).
pub fn encode_instruction(instr: &DecodedInstr) -> Vec<u8> {
    let mut out = Vec::new();
    match instr {
        DecodedInstr::Simple { op, payload } => {
            out.extend_from_slice(&pack_word(*op, *payload).to_le_bytes());
        }
        DecodedInstr::WithImm32 { op, payload, imm } => {
            out.extend_from_slice(&pack_word(*op, *payload).to_le_bytes());
            out.extend_from_slice(&imm.to_le_bytes());
        }
        DecodedInstr::CallFb {
            fb_kind,
            fb_id,
            instance_base,
        } => {
            out.extend_from_slice(&pack_word(Opcode::CallFb, 0).to_le_bytes());
            // Following u32: kind in high 8 bits, id in low 24 (implementation choice).
            let word = ((fb_kind & 0xFF) << 24) | (fb_id & 0x00FF_FFFF);
            out.extend_from_slice(&word.to_le_bytes());
            out.extend_from_slice(&instance_base.to_le_bytes());
        }
    }
    out
}

/// Decode one instruction starting at `code[pc..]` (pc in bytes). Returns (instr, bytes_consumed).
pub fn decode_instruction(code: &[u8], pc: usize) -> Result<(DecodedInstr, usize), IrError> {
    if pc + 4 > code.len() {
        return Err(IrError::Spbc(format!("truncated decode OOB at pc={pc}")));
    }
    let word = u32::from_le_bytes(code[pc..pc + 4].try_into().unwrap());
    let op_byte = (word >> 24) as u8;
    let payload = word & 0x00FF_FFFF;
    let op = Opcode::from_u8(op_byte).ok_or_else(|| IrError::Unknown {
        what: "opcode",
        name: format!("0x{op_byte:02X}"),
    })?;

    match op {
        Opcode::PushIDint | Opcode::PushIReal | Opcode::PushTime => {
            if pc + 8 > code.len() {
                return Err(IrError::Spbc("missing imm32".into()));
            }
            let imm = u32::from_le_bytes(code[pc + 4..pc + 8].try_into().unwrap());
            Ok((DecodedInstr::WithImm32 { op, payload, imm }, 8))
        }
        Opcode::CallFb => {
            if pc + 12 > code.len() {
                return Err(IrError::Spbc("CALL_FB missing operands".into()));
            }
            let w1 = u32::from_le_bytes(code[pc + 4..pc + 8].try_into().unwrap());
            let base = u32::from_le_bytes(code[pc + 8..pc + 12].try_into().unwrap());
            let fb_kind = (w1 >> 24) & 0xFF;
            let fb_id = w1 & 0x00FF_FFFF;
            Ok((
                DecodedInstr::CallFb {
                    fb_kind,
                    fb_id,
                    instance_base: base,
                },
                12,
            ))
        }
        _ => Ok((DecodedInstr::Simple { op, payload }, 4)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appendix_a8_hex_words() {
        // From architecture A.8 schematic
        assert_eq!(pack_word(Opcode::LdData, 0), 0x1000_0000);
        assert_eq!(pack_word(Opcode::LdData, 2), 0x1000_0002);
        assert_eq!(pack_word(Opcode::Or, 0), 0x2900_0000);
        assert_eq!(pack_word(Opcode::Not, 0), 0x2B00_0000);
        assert_eq!(pack_word(Opcode::And, 0), 0x2800_0000);
        assert_eq!(pack_word(Opcode::StData, 2), 0x1100_0002);
        assert_eq!(pack_word(Opcode::Ret, 0), 0x5100_0000);
    }
}
