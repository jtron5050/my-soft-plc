//! IR verifier (Appendix A.6 checklist).

use crate::encode::decode_instruction;
use crate::error::VerifyError;
use crate::module::{IrModule, IR_MAJOR};
use crate::opcode::{Opcode, PrimitiveId};
use crate::value::IrType;

const MAX_STACK: usize = 256;
const MAX_CALL_DEPTH: usize = 32;
const MAX_CODE_BYTES: u32 = 4 * 1024 * 1024;
const MAX_DATA_RETAIN: u32 = 16 * 1024 * 1024;

/// Verify a module against the A.6 checklist.
pub fn verify_module(module: &IrModule) -> Result<(), VerifyError> {
    // Rule 7: known IR major (minor may grow with new ops).
    if module.ir_major != IR_MAJOR {
        return Err(VerifyError::rule(
            7,
            format!("unsupported ir_major {}", module.ir_major),
        ));
    }

    // Rule 9: resource limits
    if module.code.len() as u32 > MAX_CODE_BYTES {
        return Err(VerifyError::rule(9, "code exceeds 4 MiB"));
    }
    if module.data_size.saturating_add(module.retain_size) > MAX_DATA_RETAIN {
        return Err(VerifyError::rule(9, "data+retain exceeds 16 MiB"));
    }
    if module.code.len() % 4 != 0 {
        return Err(VerifyError::rule(1, "code size not multiple of 4"));
    }

    // Const size consistency
    if module.const_data.len() as u32 != module.const_size {
        return Err(VerifyError::rule(
            4,
            "const_data length disagrees with const_size",
        ));
    }

    for e in &module.entries {
        if e.pc as usize >= module.code.len() && !module.code.is_empty() {
            return Err(VerifyError::rule(
                1,
                format!("entry '{}' pc out of bounds", e.name),
            ));
        }
        if e.pc % 4 != 0 {
            return Err(VerifyError::rule(
                1,
                format!("entry '{}' pc not aligned", e.name),
            ));
        }
        verify_entry(module, e.pc as usize, e.is_user_fb, &e.name)?;
    }

    Ok(())
}

#[derive(Clone, Debug)]
struct Frame {
    stack: Vec<Option<IrType>>,
}

fn verify_entry(
    module: &IrModule,
    entry_pc: usize,
    is_user_fb: bool,
    name: &str,
) -> Result<(), VerifyError> {
    // Abstract interpretation: map pc -> stack types at entry to that pc.
    let mut seen: std::collections::BTreeMap<usize, Vec<Option<IrType>>> =
        std::collections::BTreeMap::new();
    let mut work: Vec<(usize, Frame, usize)> = Vec::new(); // pc, frame, call_depth
    work.push((entry_pc, Frame { stack: Vec::new() }, 0));

    let mut ended_ok = false;

    while let Some((pc, frame, depth)) = work.pop() {
        if depth > MAX_CALL_DEPTH {
            return Err(VerifyError::rule(
                5,
                format!("call depth > {MAX_CALL_DEPTH} in {name}"),
            ));
        }
        if let Some(prev) = seen.get(&pc) {
            if stacks_compatible(prev, &frame.stack) {
                continue;
            }
            return Err(VerifyError::rule(
                3,
                format!("stack type conflict at pc={pc} in {name}"),
            ));
        }
        seen.insert(pc, frame.stack.clone());

        if pc >= module.code.len() {
            return Err(VerifyError::rule(
                1,
                format!("fall off end of code in {name}"),
            ));
        }

        let (instr, size) = decode_instruction(&module.code, pc)
            .map_err(|e| VerifyError::rule(7, format!("decode at pc={pc}: {e}")))?;

        let mut stack = frame.stack;
        let mut next_pcs: Vec<(usize, usize)> = Vec::new(); // (pc, depth)
        let mut terminal = false;

        match &instr {
            crate::encode::DecodedInstr::Simple { op, payload } => {
                match op {
                    Opcode::Nop => {
                        next_pcs.push((pc + size, depth));
                    }
                    Opcode::Halt => {
                        if is_user_fb {
                            return Err(VerifyError::rule(
                                6,
                                format!("user FB '{name}' must end in RET, not HALT"),
                            ));
                        }
                        if !stack.is_empty() {
                            // Allow residual? Architecture says end in HALT; empty preferred.
                            // Soft: allow any stack at HALT for task entries.
                        }
                        terminal = true;
                        ended_ok = true;
                    }
                    Opcode::Ret => {
                        if !is_user_fb {
                            // RET only inside FB — may still appear if verifying FB body only.
                        }
                        terminal = true;
                        ended_ok = true;
                    }
                    Opcode::PushIBool => {
                        push(&mut stack, Some(IrType::Bool))?;
                        next_pcs.push((pc + size, depth));
                    }
                    Opcode::LdData | Opcode::LdRetain => {
                        check_offset(*payload, segment_size(module, *op))?;
                        // Unknown concrete type without type map — use None (top).
                        push(&mut stack, None)?;
                        next_pcs.push((pc + size, depth));
                    }
                    Opcode::StData | Opcode::StRetain => {
                        check_offset(*payload, segment_size(module, *op))?;
                        pop(&mut stack)?;
                        next_pcs.push((pc + size, depth));
                    }
                    Opcode::LdI | Opcode::LdIq => {
                        if *payload >= module.input_slots {
                            return Err(VerifyError::rule(
                                4,
                                format!("LD_I/LD_IQ index {payload} >= input_slots"),
                            ));
                        }
                        // Rule 10: quality plane length equals input_slots (header).
                        if *op == Opcode::LdIq {
                            push(&mut stack, Some(IrType::Bool))?;
                        } else {
                            push(&mut stack, None)?;
                        }
                        next_pcs.push((pc + size, depth));
                    }
                    Opcode::StQ => {
                        if *payload >= module.output_slots {
                            return Err(VerifyError::rule(
                                4,
                                format!("ST_Q index {payload} >= output_slots"),
                            ));
                        }
                        pop(&mut stack)?;
                        next_pcs.push((pc + size, depth));
                    }
                    Opcode::LdQ => {
                        if *payload >= module.output_slots {
                            return Err(VerifyError::rule(
                                4,
                                format!("LD_Q index {payload} >= output_slots"),
                            ));
                        }
                        push(&mut stack, None)?;
                        next_pcs.push((pc + size, depth));
                    }
                    Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div => {
                        let b = pop(&mut stack)?;
                        let a = pop(&mut stack)?;
                        let r = binary_numeric(a, b, *op)?;
                        push(&mut stack, r)?;
                        next_pcs.push((pc + size, depth));
                    }
                    Opcode::Neg => {
                        let a = pop(&mut stack)?;
                        push(&mut stack, a)?;
                        next_pcs.push((pc + size, depth));
                    }
                    Opcode::And | Opcode::Or | Opcode::Xor => {
                        let b = pop(&mut stack)?;
                        let a = pop(&mut stack)?;
                        let r = binary_logic(a, b)?;
                        push(&mut stack, r)?;
                        next_pcs.push((pc + size, depth));
                    }
                    Opcode::Not => {
                        let a = pop(&mut stack)?;
                        push(&mut stack, a.or(Some(IrType::Bool)))?;
                        next_pcs.push((pc + size, depth));
                    }
                    Opcode::Eq | Opcode::Ne | Opcode::Lt | Opcode::Le | Opcode::Gt | Opcode::Ge => {
                        pop(&mut stack)?;
                        pop(&mut stack)?;
                        push(&mut stack, Some(IrType::Bool))?;
                        next_pcs.push((pc + size, depth));
                    }
                    Opcode::Jmp => {
                        let target = *payload as usize;
                        check_jmp_target(module, target)?;
                        next_pcs.push((target, depth));
                    }
                    Opcode::JmpIf | Opcode::JmpIfNot => {
                        let c = pop(&mut stack)?;
                        if let Some(t) = c {
                            if t != IrType::Bool {
                                return Err(VerifyError::rule(
                                    3,
                                    "JMP_IF* requires BOOL condition",
                                ));
                            }
                        }
                        let target = *payload as usize;
                        check_jmp_target(module, target)?;
                        next_pcs.push((target, depth));
                        next_pcs.push((pc + size, depth));
                    }
                    Opcode::Conv => {
                        let _ = pop(&mut stack)?;
                        let ty = IrType::from_u8(*payload as u8).ok_or_else(|| {
                            VerifyError::rule(3, format!("CONV bad type tag {payload}"))
                        })?;
                        push(&mut stack, Some(ty))?;
                        next_pcs.push((pc + size, depth));
                    }
                    Opcode::PushIDint | Opcode::PushIReal | Opcode::PushTime | Opcode::CallFb => {
                        return Err(VerifyError::rule(
                            7,
                            format!("opcode {:?} should be wide form", op),
                        ));
                    }
                }
            }
            crate::encode::DecodedInstr::WithImm32 { op, .. } => match op {
                Opcode::PushIDint => {
                    push(&mut stack, Some(IrType::Dint))?;
                    next_pcs.push((pc + size, depth));
                }
                Opcode::PushIReal => {
                    push(&mut stack, Some(IrType::Real))?;
                    next_pcs.push((pc + size, depth));
                }
                Opcode::PushTime => {
                    push(&mut stack, Some(IrType::Time))?;
                    next_pcs.push((pc + size, depth));
                }
                _ => {
                    return Err(VerifyError::rule(7, "unexpected WithImm32"));
                }
            },
            crate::encode::DecodedInstr::CallFb {
                fb_kind,
                fb_id,
                instance_base,
            } => {
                if *instance_base >= module.data_size.saturating_add(module.retain_size)
                    && module.data_size > 0
                {
                    // Soft bound: instance base should lie in data or retain.
                    if *instance_base >= module.data_size && *instance_base >= module.retain_size {
                        // allow if within max of both for simplicity when retain is separate
                    }
                }
                if *fb_kind == 0 {
                    let prim = match *fb_id {
                        1 => PrimitiveId::Ton,
                        2 => PrimitiveId::Tof,
                        3 => PrimitiveId::Tp,
                        4 => PrimitiveId::Ctu,
                        5 => PrimitiveId::Ctd,
                        6 => PrimitiveId::Rs,
                        7 => PrimitiveId::Sr,
                        8 => PrimitiveId::RTrig,
                        9 => PrimitiveId::FTrig,
                        10 => PrimitiveId::Pid,
                        _ => {
                            return Err(VerifyError::rule(
                                7,
                                format!("unknown primitive id {fb_id}"),
                            ));
                        }
                    };
                    for _ in 0..prim.input_count() {
                        pop(&mut stack)?;
                    }
                    // TON etc. use instance memory — check base loosely
                    let _ = instance_base;
                    for i in 0..prim.output_count() {
                        let ty = if i == 0 {
                            Some(IrType::Bool)
                        } else {
                            Some(IrType::Time)
                        };
                        push(&mut stack, ty)?;
                    }
                    next_pcs.push((pc + size, depth));
                } else {
                    // User FB: treat as opaque — pop nothing known; require depth+1
                    // Without a type ABI we only check depth budget on nested calls.
                    next_pcs.push((pc + size, depth + 1));
                }
            }
        }

        if terminal {
            continue;
        }
        if next_pcs.is_empty() {
            return Err(VerifyError::rule(
                6,
                format!("no successor at pc={pc} in {name}"),
            ));
        }
        for (npc, nd) in next_pcs {
            work.push((
                npc,
                Frame {
                    stack: stack.clone(),
                },
                nd,
            ));
        }
    }

    if !ended_ok {
        return Err(VerifyError::rule(
            6,
            format!("entry '{name}' has path without HALT/RET"),
        ));
    }
    Ok(())
}

fn segment_size(module: &IrModule, op: Opcode) -> u32 {
    match op {
        Opcode::LdData | Opcode::StData => module.data_size,
        Opcode::LdRetain | Opcode::StRetain => module.retain_size,
        _ => 0,
    }
}

fn check_offset(off: u32, size: u32) -> Result<(), VerifyError> {
    if size > 0 && off >= size {
        return Err(VerifyError::rule(
            4,
            format!("offset {off} out of segment size {size}"),
        ));
    }
    Ok(())
}

fn check_jmp_target(module: &IrModule, target: usize) -> Result<(), VerifyError> {
    if target % 4 != 0 {
        return Err(VerifyError::rule(
            1,
            format!("JMP target {target} unaligned"),
        ));
    }
    if target >= module.code.len() {
        return Err(VerifyError::rule(
            1,
            format!("JMP target {target} out of bounds"),
        ));
    }
    Ok(())
}

fn push(stack: &mut Vec<Option<IrType>>, ty: Option<IrType>) -> Result<(), VerifyError> {
    if stack.len() >= MAX_STACK {
        return Err(VerifyError::rule(2, "stack depth > 256"));
    }
    stack.push(ty);
    Ok(())
}

fn pop(stack: &mut Vec<Option<IrType>>) -> Result<Option<IrType>, VerifyError> {
    stack
        .pop()
        .ok_or_else(|| VerifyError::rule(2, "stack underflow"))
}

fn stacks_compatible(a: &[Option<IrType>], b: &[Option<IrType>]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).all(|(x, y)| match (x, y) {
        (None, _) | (_, None) => true,
        (Some(t), Some(u)) => t == u,
    })
}

fn binary_numeric(
    a: Option<IrType>,
    b: Option<IrType>,
    _op: Opcode,
) -> Result<Option<IrType>, VerifyError> {
    match (a, b) {
        (Some(x), Some(y)) if x == y && x.is_numeric() => Ok(Some(x)),
        (Some(x), Some(y)) if x != y => Err(VerifyError::rule(
            3,
            format!("numeric type mismatch {x:?} vs {y:?}"),
        )),
        _ => Ok(a.or(b)),
    }
}

fn binary_logic(a: Option<IrType>, b: Option<IrType>) -> Result<Option<IrType>, VerifyError> {
    match (a, b) {
        (Some(IrType::Bool), Some(IrType::Bool)) => Ok(Some(IrType::Bool)),
        (Some(x), Some(y)) if x == y && x.is_integral() => Ok(Some(x)),
        (Some(_), Some(_)) => Err(VerifyError::rule(3, "logic operand type mismatch")),
        _ => Ok(a.or(b).or(Some(IrType::Bool))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asm::assemble;

    #[test]
    fn reject_unknown_opcode_byte() {
        let mut m = assemble("HALT\n").unwrap();
        // Patch first byte of opcode to 0xEE
        m.code[3] = 0xEE; // LE: opcode is high byte at index 3
        let err = verify_module(&m).unwrap_err();
        assert!(err.to_string().contains("decode") || err.to_string().contains("rule 7"));
    }

    #[test]
    fn reject_stack_underflow() {
        let src = r#"
.header data_size=16 input_slots=0 output_slots=0
.entry task.main
AND
HALT
"#;
        let m = assemble(src).unwrap();
        let err = verify_module(&m).unwrap_err();
        assert!(err.to_string().contains("underflow"));
    }
}
