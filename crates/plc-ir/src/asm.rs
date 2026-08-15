//! Human-readable `spasm` text assembler.

use std::collections::HashMap;

use crate::encode::{encode_instruction, pack_word, DecodedInstr};
use crate::error::IrError;
use crate::module::{EntryPoint, IrModule, IR_MAJOR, IR_MINOR};
use crate::opcode::{Opcode, PrimitiveId};

/// Options for assembly (defaults match Appendix A pilot).
#[derive(Debug, Clone)]
pub struct AssembleOptions {
    /// Default data segment size if not set in `.header`.
    pub default_data_size: u32,
    /// Default retain size.
    pub default_retain_size: u32,
    /// Default input slots.
    pub default_input_slots: u32,
    /// Default output slots.
    pub default_output_slots: u32,
}

impl Default for AssembleOptions {
    fn default() -> Self {
        Self {
            default_data_size: 256,
            default_retain_size: 0,
            default_input_slots: 16,
            default_output_slots: 16,
        }
    }
}

/// Assemble `spasm` source into an [`IrModule`].
pub fn assemble(source: &str) -> Result<IrModule, IrError> {
    assemble_with(source, &AssembleOptions::default())
}

/// Assemble with options.
pub fn assemble_with(source: &str, opts: &AssembleOptions) -> Result<IrModule, IrError> {
    let mut data_size = opts.default_data_size;
    let mut retain_size = opts.default_retain_size;
    let mut input_slots = opts.default_input_slots;
    let mut output_slots = opts.default_output_slots;
    let mut const_size_decl: Option<u32> = None;

    let mut entries: Vec<EntryPoint> = Vec::new();
    let mut labels: HashMap<String, u32> = HashMap::new();
    let mut code: Vec<u8> = Vec::new();
    // Pending fixups: (code_offset_of_word, label)
    let mut fixups: Vec<(usize, String, usize)> = Vec::new(); // offset, label, line

    // First pass: strip comments, collect lines with numbers.
    let lines: Vec<(usize, String)> = source
        .lines()
        .enumerate()
        .map(|(i, l)| {
            let line_no = i + 1;
            let without_comment = l.split(';').next().unwrap_or("").trim();
            (line_no, without_comment.to_string())
        })
        .filter(|(_, l)| !l.is_empty())
        .collect();

    for (line_no, line) in &lines {
        if let Some(rest) = line.strip_prefix('.') {
            // Directive
            let mut parts = rest.split_whitespace();
            let dir = parts.next().unwrap_or("");
            match dir {
                "header" => {
                    for kv in parts {
                        let mut it = kv.splitn(2, '=');
                        let k = it.next().unwrap_or("");
                        let v = it.next().ok_or_else(|| IrError::Asm {
                            line: *line_no,
                            message: format!("header expected key=value, got {kv}"),
                        })?;
                        let n = parse_int(v).map_err(|m| IrError::Asm {
                            line: *line_no,
                            message: m,
                        })?;
                        let n = u32::try_from(n).map_err(|_| IrError::Asm {
                            line: *line_no,
                            message: format!("value out of u32 range: {v}"),
                        })?;
                        match k {
                            "data_size" => data_size = n,
                            "retain_size" => retain_size = n,
                            "input_slots" => input_slots = n,
                            "output_slots" => output_slots = n,
                            "const_size" => const_size_decl = Some(n),
                            other => {
                                return Err(IrError::Asm {
                                    line: *line_no,
                                    message: format!("unknown header field {other}"),
                                });
                            }
                        }
                    }
                }
                "entry" => {
                    let name = parts.next().ok_or_else(|| IrError::Asm {
                        line: *line_no,
                        message: ".entry requires a name".into(),
                    })?;
                    let kind = parts.next().unwrap_or("");
                    let is_user_fb = kind.eq_ignore_ascii_case("fb")
                        || name.starts_with("fb.")
                        || name.starts_with("FB.");
                    let pc = code.len() as u32;
                    entries.push(EntryPoint {
                        name: name.to_string(),
                        pc,
                        is_user_fb,
                    });
                }
                other => {
                    return Err(IrError::Asm {
                        line: *line_no,
                        message: format!("unknown directive .{other}"),
                    });
                }
            }
            continue;
        }

        // Label: ends with ':'
        if let Some(label) = line.strip_suffix(':') {
            let label = label.trim();
            if label.is_empty() {
                return Err(IrError::Asm {
                    line: *line_no,
                    message: "empty label".into(),
                });
            }
            labels.insert(label.to_string(), code.len() as u32);
            continue;
        }

        // Instruction
        let mut toks = tokenize(line);
        if toks.is_empty() {
            continue;
        }
        let mnem = toks.remove(0);
        let op = Opcode::from_mnemonic(&mnem).ok_or_else(|| IrError::Asm {
            line: *line_no,
            message: format!("unknown mnemonic {mnem}"),
        })?;

        match op {
            Opcode::PushIDint | Opcode::PushTime => {
                let imm = expect_one(&toks, *line_no)?;
                let v = parse_int(&imm).map_err(|m| IrError::Asm {
                    line: *line_no,
                    message: m,
                })?;
                let instr = DecodedInstr::WithImm32 {
                    op,
                    payload: 0,
                    imm: v as u32,
                };
                code.extend_from_slice(&encode_instruction(&instr));
            }
            Opcode::PushIReal => {
                let imm = expect_one(&toks, *line_no)?;
                let f: f32 = imm.parse().map_err(|_| IrError::Asm {
                    line: *line_no,
                    message: format!("bad real immediate {imm}"),
                })?;
                let instr = DecodedInstr::WithImm32 {
                    op,
                    payload: 0,
                    imm: f.to_bits(),
                };
                code.extend_from_slice(&encode_instruction(&instr));
            }
            Opcode::PushIBool => {
                let imm = expect_one(&toks, *line_no)?;
                let b = parse_bool(&imm).map_err(|m| IrError::Asm {
                    line: *line_no,
                    message: m,
                })?;
                code.extend_from_slice(&pack_word(op, u32::from(b)).to_le_bytes());
            }
            Opcode::LdData
            | Opcode::StData
            | Opcode::LdRetain
            | Opcode::StRetain
            | Opcode::LdI
            | Opcode::StQ
            | Opcode::LdQ
            | Opcode::LdIq
            | Opcode::Conv => {
                let imm = expect_one(&toks, *line_no)?;
                let v = if op == Opcode::Conv {
                    parse_type_tag(&imm).map_err(|m| IrError::Asm {
                        line: *line_no,
                        message: m,
                    })?
                } else {
                    parse_int(&imm).map_err(|m| IrError::Asm {
                        line: *line_no,
                        message: m,
                    })? as u32
                };
                code.extend_from_slice(&pack_word(op, v).to_le_bytes());
            }
            Opcode::Jmp | Opcode::JmpIf | Opcode::JmpIfNot => {
                let target = expect_one(&toks, *line_no)?;
                if let Ok(abs) = parse_int(&target) {
                    code.extend_from_slice(&pack_word(op, abs as u32).to_le_bytes());
                } else {
                    let off = code.len();
                    code.extend_from_slice(&pack_word(op, 0).to_le_bytes());
                    fixups.push((off, target, *line_no));
                }
            }
            Opcode::CallFb => {
                // Forms:
                //   CALL_FB prim=TON instance=0x40
                //   CALL_FB TON 0x40
                //   CALL_FB user=1 instance=0x10
                let (kind, id, base) = parse_call_fb(&toks, *line_no)?;
                let instr = DecodedInstr::CallFb {
                    fb_kind: kind,
                    fb_id: id,
                    instance_base: base,
                };
                code.extend_from_slice(&encode_instruction(&instr));
            }
            Opcode::Nop
            | Opcode::Halt
            | Opcode::Add
            | Opcode::Sub
            | Opcode::Mul
            | Opcode::Div
            | Opcode::Neg
            | Opcode::And
            | Opcode::Or
            | Opcode::Xor
            | Opcode::Not
            | Opcode::Eq
            | Opcode::Ne
            | Opcode::Lt
            | Opcode::Le
            | Opcode::Gt
            | Opcode::Ge
            | Opcode::Ret => {
                if !toks.is_empty() {
                    return Err(IrError::Asm {
                        line: *line_no,
                        message: format!("{mnem} takes no operands"),
                    });
                }
                code.extend_from_slice(&pack_word(op, 0).to_le_bytes());
            }
        }
    }

    // Apply label fixups (absolute byte PC).
    for (off, label, line_no) in fixups {
        let pc = *labels.get(&label).ok_or_else(|| IrError::Asm {
            line: line_no,
            message: format!("undefined label {label}"),
        })?;
        if pc % 4 != 0 {
            return Err(IrError::Asm {
                line: line_no,
                message: format!("label {label} not word-aligned"),
            });
        }
        // Recover opcode from existing word.
        let word = u32::from_le_bytes(code[off..off + 4].try_into().unwrap());
        let op_byte = (word >> 24) as u8;
        let new_word = (u32::from(op_byte) << 24) | (pc & 0x00FF_FFFF);
        code[off..off + 4].copy_from_slice(&new_word.to_le_bytes());
    }

    if entries.is_empty() {
        // Implicit single task entry at 0.
        entries.push(EntryPoint {
            name: "task.main".into(),
            pc: 0,
            is_user_fb: false,
        });
    }

    let const_data = vec![0u8; const_size_decl.unwrap_or(0) as usize];

    Ok(IrModule {
        ir_major: IR_MAJOR,
        ir_minor: IR_MINOR,
        const_size: const_data.len() as u32,
        data_size,
        retain_size,
        input_slots,
        output_slots,
        entries,
        const_data,
        code,
    })
}

fn tokenize(line: &str) -> Vec<String> {
    // Split on whitespace and commas.
    line.split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .map(std::string::ToString::to_string)
        .collect()
}

fn expect_one(toks: &[String], line: usize) -> Result<String, IrError> {
    if toks.len() != 1 {
        return Err(IrError::Asm {
            line,
            message: format!("expected 1 operand, found {}", toks.len()),
        });
    }
    Ok(toks[0].clone())
}

fn parse_int(s: &str) -> Result<i64, String> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).map_err(|_| format!("bad hex integer {s}"))
    } else {
        s.parse::<i64>().map_err(|_| format!("bad integer {s}"))
    }
}

fn parse_bool(s: &str) -> Result<bool, String> {
    match s.to_ascii_lowercase().as_str() {
        "1" | "true" | "on" => Ok(true),
        "0" | "false" | "off" => Ok(false),
        _ => Err(format!("bad bool {s}")),
    }
}

fn parse_type_tag(s: &str) -> Result<u32, String> {
    match s.to_ascii_uppercase().as_str() {
        "BOOL" | "0" => Ok(0),
        "INT" | "1" => Ok(1),
        "DINT" | "2" => Ok(2),
        "REAL" | "3" => Ok(3),
        "TIME" | "4" => Ok(4),
        "LINT" | "5" => Ok(5),
        _ => Err(format!("bad type tag {s}")),
    }
}

fn parse_call_fb(toks: &[String], line: usize) -> Result<(u32, u32, u32), IrError> {
    let mut kind: Option<u32> = None;
    let mut id: Option<u32> = None;
    let mut base: Option<u32> = None;

    for t in toks {
        if let Some(v) = t.strip_prefix("prim=") {
            let p = PrimitiveId::from_name(v).ok_or_else(|| IrError::Asm {
                line,
                message: format!("unknown primitive {v}"),
            })?;
            kind = Some(0);
            id = Some(p as u32);
        } else if let Some(v) = t.strip_prefix("user=") {
            kind = Some(1);
            id = Some(parse_int(v).map_err(|m| IrError::Asm { line, message: m })? as u32);
        } else if let Some(v) = t
            .strip_prefix("instance=")
            .or_else(|| t.strip_prefix("inst="))
        {
            base = Some(parse_int(v).map_err(|m| IrError::Asm { line, message: m })? as u32);
        } else if let Some(p) = PrimitiveId::from_name(t) {
            kind = Some(0);
            id = Some(p as u32);
        } else if kind.is_some() && base.is_none() {
            base = Some(parse_int(t).map_err(|m| IrError::Asm { line, message: m })? as u32);
        } else {
            return Err(IrError::Asm {
                line,
                message: format!("unexpected CALL_FB token {t}"),
            });
        }
    }

    let kind = kind.ok_or_else(|| IrError::Asm {
        line,
        message: "CALL_FB missing primitive/user id".into(),
    })?;
    let id = id.ok_or_else(|| IrError::Asm {
        line,
        message: "CALL_FB missing id".into(),
    })?;
    let base = base.ok_or_else(|| IrError::Asm {
        line,
        message: "CALL_FB missing instance base".into(),
    })?;
    Ok((kind, id, base))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::decode_instruction;
    use crate::verify::verify_module;

    #[test]
    fn assemble_rs_fixture() {
        let src = include_str!("../tests/fixtures/rs_latch.spasm");
        let m = assemble(src).expect("assemble");
        verify_module(&m).expect("verify");
        // First word LD_DATA 0
        let (ins, _) = decode_instruction(&m.code, 0).unwrap();
        assert!(matches!(
            ins,
            DecodedInstr::Simple {
                op: Opcode::LdData,
                payload: 0
            }
        ));
    }

    #[test]
    fn assemble_ton_fixture() {
        let src = include_str!("../tests/fixtures/ton_call.spasm");
        let m = assemble(src).expect("assemble");
        verify_module(&m).expect("verify");
    }
}
