//! Interpreter core — no heap allocation in the run loop.

use plc_fb_primitives::{call_primitive, PrimitiveStore, StackValue};
use plc_ir::{
    decode_instruction, verify_module, DecodedInstr, IrModule, IrType, Opcode, PrimitiveId,
};

use crate::error::VmError;
use crate::load::VmConfig;
use crate::memory::{ByteSegment, SlotImage};
use crate::value::VmValue;
use crate::{MAX_CALL_DEPTH, MAX_STACK};

/// Outcome of a successful `run_entry`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecResult {
    /// Task entry ended at HALT.
    Halted,
    /// User FB body returned (should not be the top-level task result).
    Returned,
}

#[derive(Debug, Clone, Copy)]
struct Frame {
    return_pc: usize,
    /// Base added to LD_DATA/ST_DATA/LD_RETAIN/ST_RETAIN offsets inside this FB.
    data_base: u32,
}

/// Armed IR virtual machine instance.
pub struct Vm {
    code: Vec<u8>,
    #[allow(dead_code)]
    const_data: Vec<u8>,
    data: ByteSegment,
    retain: ByteSegment,
    inputs: SlotImage,
    outputs: SlotImage,
    entries: Vec<(String, u32, bool)>,
    /// user_fb_id → code PC (byte).
    user_fb_pc: Vec<Option<u32>>,
    primitives: PrimitiveStore,
    stack: [VmValue; MAX_STACK],
    sp: usize,
    frames: [Frame; MAX_CALL_DEPTH],
    fp: usize,
    /// Current data base (0 at task level; set from top frame inside FB).
    data_base: u32,
    instruction_budget: u64,
    /// Runtime diagnostic: DIV by zero count.
    pub div0_count: u32,
    /// Set when any ST_RETAIN executes this run (or since clear).
    pub retain_dirty: bool,
    /// `SYSTEM.FirstScan` for the current invocation (set by the scan engine).
    first_scan: bool,
}

impl Vm {
    /// Verify (optional), allocate image, and arm a module.
    pub fn load(module: IrModule, config: &VmConfig) -> Result<Self, VmError> {
        if config.verify {
            verify_module(&module).map_err(|e| VmError::Verify(e.to_string()))?;
        }

        let mut user_fb_pc = Vec::new();
        let mut entries = Vec::with_capacity(module.entries.len());
        for e in &module.entries {
            entries.push((e.name.clone(), e.pc, e.is_user_fb));
            if e.is_user_fb {
                // Map sequential user FB registration: also allow numeric suffix.
                let id = parse_user_fb_id(&e.name).unwrap_or(user_fb_pc.len() as u32);
                let idx = id as usize;
                if user_fb_pc.len() <= idx {
                    user_fb_pc.resize(idx + 1, None);
                }
                user_fb_pc[idx] = Some(e.pc);
            }
        }

        Ok(Self {
            code: module.code,
            const_data: module.const_data,
            data: ByteSegment::zeros(module.data_size as usize),
            retain: ByteSegment::zeros(module.retain_size as usize),
            inputs: SlotImage::bools(module.input_slots as usize),
            outputs: SlotImage::bools(module.output_slots as usize),
            entries,
            user_fb_pc,
            primitives: PrimitiveStore::with_capacity(config.primitive_instances),
            stack: [VmValue::Bool(false); MAX_STACK],
            sp: 0,
            frames: [Frame {
                return_pc: 0,
                data_base: 0,
            }; MAX_CALL_DEPTH],
            fp: 0,
            data_base: 0,
            instruction_budget: config.instruction_budget,
            div0_count: 0,
            retain_dirty: false,
            first_scan: false,
        })
    }

    /// Assemble from `spasm` source and load.
    pub fn from_spasm(source: &str, config: &VmConfig) -> Result<Self, VmError> {
        let module = plc_ir::assemble(source).map_err(|e| VmError::Verify(e.to_string()))?;
        Self::load(module, config)
    }

    /// Cold-reset non-retain state (data, outputs, primitives, stack); retain kept.
    pub fn cold_reset_non_retain(&mut self) {
        let n = self.data.len();
        self.data = ByteSegment::zeros(n);
        let nq = self.outputs.len();
        self.outputs = SlotImage::bools(nq);
        self.primitives.cold_reset_all();
        self.sp = 0;
        self.fp = 0;
        self.data_base = 0;
        self.div0_count = 0;
    }

    /// Clear retain-dirty flag (after flush).
    pub fn clear_retain_dirty(&mut self) {
        self.retain_dirty = false;
    }

    /// Look up entry PC by name.
    pub fn entry_pc(&self, name: &str) -> Result<usize, VmError> {
        self.entries
            .iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, pc, _)| *pc as usize)
            .ok_or_else(|| VmError::UnknownEntry(name.to_string()))
    }

    /// Data segment (tests / scan glue).
    #[must_use]
    pub fn data(&self) -> &ByteSegment {
        &self.data
    }

    /// Mutable data segment.
    pub fn data_mut(&mut self) -> &mut ByteSegment {
        &mut self.data
    }

    /// Retain segment.
    #[must_use]
    pub fn retain(&self) -> &ByteSegment {
        &self.retain
    }

    /// Mutable retain segment (arm-time shadow install / tests).
    pub fn retain_mut(&mut self) -> &mut ByteSegment {
        &mut self.retain
    }

    /// Copy a byte range from `src`'s retain segment into this retain image.
    pub fn blit_retain_from(
        &mut self,
        dst: usize,
        src: &Self,
        src_off: usize,
        len: usize,
    ) -> Result<(), VmError> {
        self.retain.blit_from(dst, &src.retain, src_off, len)
    }

    /// Tag and store a packed retain image using `layout` (non-RT arm path).
    pub fn load_retain_image(
        &mut self,
        image: &[u8],
        layout: &plc_ir::RetainLayout,
    ) -> Result<(), VmError> {
        if image.len() != self.retain.len() {
            return Err(VmError::Bounds {
                pc: 0,
                detail: format!(
                    "retain image {} != segment {}",
                    image.len(),
                    self.retain.len()
                ),
            });
        }
        if layout.retain_size as usize != image.len() {
            return Err(VmError::Bounds {
                pc: 0,
                detail: format!(
                    "retain layout size {} != image {}",
                    layout.retain_size,
                    image.len()
                ),
            });
        }
        for sym in &layout.symbols {
            let off = sym.offset as usize;
            let width = sym.ty.byte_width();
            let end = off.saturating_add(width);
            if end > image.len() {
                return Err(VmError::Bounds {
                    pc: 0,
                    detail: format!("retain symbol {} OOB", sym.name),
                });
            }
            let v = VmValue::from_le_bytes(sym.ty, &image[off..end])?;
            self.retain.store(off, v, 0)?;
        }
        Ok(())
    }

    /// `SYSTEM.FirstScan` as seen by the current invocation.
    #[must_use]
    pub fn first_scan(&self) -> bool {
        self.first_scan
    }

    /// Scan engine: publish the per-task FirstScan bit for this invocation.
    pub fn set_first_scan(&mut self, v: bool) {
        self.first_scan = v;
    }

    /// Inputs image.
    #[must_use]
    pub fn inputs(&self) -> &SlotImage {
        &self.inputs
    }

    /// Mutable inputs.
    pub fn inputs_mut(&mut self) -> &mut SlotImage {
        &mut self.inputs
    }

    /// Outputs image.
    #[must_use]
    pub fn outputs(&self) -> &SlotImage {
        &self.outputs
    }

    /// Mutable outputs.
    pub fn outputs_mut(&mut self) -> &mut SlotImage {
        &mut self.outputs
    }

    /// Primitive store (tests / hot-swap policy).
    #[must_use]
    pub fn primitives(&self) -> &PrimitiveStore {
        &self.primitives
    }

    /// Mutable primitive store (tests).
    pub fn primitives_mut(&mut self) -> &mut PrimitiveStore {
        &mut self.primitives
    }

    /// Run a named entry with monotonic `now_ms` for timers/PID.
    pub fn run_entry(&mut self, name: &str, now_ms: u64) -> Result<ExecResult, VmError> {
        let pc = self.entry_pc(name)?;
        self.run_at(pc, now_ms)
    }

    /// Run from a byte PC until HALT (task) or RET that empties frames (FB).
    pub fn run_at(&mut self, start_pc: usize, now_ms: u64) -> Result<ExecResult, VmError> {
        // Reset operand/call stack for a fresh invocation — no heap ops.
        self.sp = 0;
        self.fp = 0;
        self.data_base = 0;

        let mut pc = start_pc;
        let mut steps = 0u64;

        loop {
            steps += 1;
            if steps > self.instruction_budget {
                return Err(VmError::Budget(self.instruction_budget));
            }

            let (instr, size) =
                decode_instruction(&self.code, pc).map_err(|e| VmError::Decode {
                    pc,
                    detail: e.to_string(),
                })?;

            match instr {
                DecodedInstr::Simple { op, payload } => match op {
                    Opcode::Nop => {
                        pc += size;
                    }
                    Opcode::Halt => {
                        return Ok(ExecResult::Halted);
                    }
                    Opcode::Ret => {
                        // Top-level user FB body (run as entry): RET returns to host.
                        if self.fp == 0 {
                            return Ok(ExecResult::Returned);
                        }
                        self.fp -= 1;
                        let frame = self.frames[self.fp];
                        pc = frame.return_pc;
                        self.data_base = if self.fp == 0 {
                            0
                        } else {
                            self.frames[self.fp - 1].data_base
                        };
                    }
                    Opcode::PushIBool => {
                        self.push(VmValue::Bool(payload != 0), pc)?;
                        pc += size;
                    }
                    Opcode::LdData => {
                        let off = self.data_base.saturating_add(payload) as usize;
                        let v = self.data.load(off, pc)?;
                        self.push(v, pc)?;
                        pc += size;
                    }
                    Opcode::StData => {
                        let v = self.pop(pc)?;
                        let off = self.data_base.saturating_add(payload) as usize;
                        self.data.store(off, v, pc)?;
                        pc += size;
                    }
                    Opcode::LdRetain => {
                        let off = self.data_base.saturating_add(payload) as usize;
                        let v = self.retain.load(off, pc)?;
                        self.push(v, pc)?;
                        pc += size;
                    }
                    Opcode::StRetain => {
                        let v = self.pop(pc)?;
                        let off = self.data_base.saturating_add(payload) as usize;
                        self.retain.store(off, v, pc)?;
                        self.retain_dirty = true;
                        pc += size;
                    }
                    Opcode::LdI => {
                        let v = self.inputs.get(payload as usize, pc)?;
                        self.push(v, pc)?;
                        pc += size;
                    }
                    Opcode::StQ => {
                        let v = self.pop(pc)?;
                        self.outputs.set(payload as usize, v, pc)?;
                        pc += size;
                    }
                    Opcode::LdQ => {
                        let v = self.outputs.get(payload as usize, pc)?;
                        self.push(v, pc)?;
                        pc += size;
                    }
                    Opcode::LdIq => {
                        let g = self.inputs.quality_good(payload as usize, pc)?;
                        self.push(VmValue::Bool(g), pc)?;
                        pc += size;
                    }
                    Opcode::Add
                    | Opcode::Sub
                    | Opcode::Mul
                    | Opcode::Div
                    | Opcode::And
                    | Opcode::Or
                    | Opcode::Xor
                    | Opcode::Eq
                    | Opcode::Ne
                    | Opcode::Lt
                    | Opcode::Le
                    | Opcode::Gt
                    | Opcode::Ge => {
                        let b = self.pop(pc)?;
                        let a = self.pop(pc)?;
                        let r = self.binop(op, a, b, pc)?;
                        self.push(r, pc)?;
                        pc += size;
                    }
                    Opcode::Neg | Opcode::Not => {
                        let a = self.pop(pc)?;
                        let r = unary_op(op, a, pc)?;
                        self.push(r, pc)?;
                        pc += size;
                    }
                    Opcode::Jmp => {
                        pc = payload as usize;
                    }
                    Opcode::JmpIf => {
                        let c = self.pop(pc)?;
                        if c.as_bool() {
                            pc = payload as usize;
                        } else {
                            pc += size;
                        }
                    }
                    Opcode::JmpIfNot => {
                        let c = self.pop(pc)?;
                        if !c.as_bool() {
                            pc = payload as usize;
                        } else {
                            pc += size;
                        }
                    }
                    Opcode::Conv => {
                        let a = self.pop(pc)?;
                        let ty = IrType::from_u8(payload as u8).ok_or_else(|| VmError::Type {
                            pc,
                            detail: format!("bad CONV tag {payload}"),
                        })?;
                        self.push(a.convert(ty), pc)?;
                        pc += size;
                    }
                    Opcode::PushIDint | Opcode::PushIReal | Opcode::PushTime | Opcode::CallFb => {
                        return Err(VmError::Decode {
                            pc,
                            detail: format!("opcode {op:?} expected wide form"),
                        });
                    }
                },
                DecodedInstr::WithImm32 { op, imm, .. } => match op {
                    Opcode::PushIDint => {
                        self.push(VmValue::Dint(imm as i32), pc)?;
                        pc += size;
                    }
                    Opcode::PushIReal => {
                        self.push(VmValue::Real(f32::from_bits(imm)), pc)?;
                        pc += size;
                    }
                    Opcode::PushTime => {
                        self.push(VmValue::Time(imm as i32), pc)?;
                        pc += size;
                    }
                    _ => {
                        return Err(VmError::Decode {
                            pc,
                            detail: "unexpected WithImm32".into(),
                        });
                    }
                },
                DecodedInstr::CallFb {
                    fb_kind,
                    fb_id,
                    instance_base,
                } => {
                    if fb_kind == 0 {
                        self.call_prim(fb_id, instance_base as usize, now_ms, pc)?;
                        pc += size;
                    } else {
                        // User FB: pop nothing (args already on stack or in instance data).
                        let entry = self
                            .user_fb_pc
                            .get(fb_id as usize)
                            .copied()
                            .flatten()
                            .ok_or(VmError::UnknownUserFb(fb_id))?;
                        if self.fp >= MAX_CALL_DEPTH {
                            return Err(VmError::CallDepth { pc });
                        }
                        self.frames[self.fp] = Frame {
                            return_pc: pc + size,
                            data_base: instance_base,
                        };
                        self.fp += 1;
                        self.data_base = instance_base;
                        pc = entry as usize;
                    }
                }
            }
        }
    }

    fn call_prim(
        &mut self,
        fb_id: u32,
        instance: usize,
        now_ms: u64,
        pc: usize,
    ) -> Result<(), VmError> {
        let id = match fb_id {
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
                return Err(VmError::Primitive {
                    pc,
                    detail: format!("unknown primitive id {fb_id}"),
                });
            }
        };
        let n = id.input_count() as usize;
        // Pop last-to-first (top is last input).
        let mut inputs = [StackValue::Bool(false); 4];
        for i in (0..n).rev() {
            let v = self.pop(pc)?;
            inputs[i] = v.to_stack_value();
        }
        let out = call_primitive(&mut self.primitives, id, instance, &inputs[..n], now_ms)
            .map_err(|e| VmError::Primitive {
                pc,
                detail: format!("{e:?}"),
            })?;
        // Push so first output is on top (matches ST_DATA of Q then ET).
        for i in (0..out.count as usize).rev() {
            let prefer_time =
                matches!(id, PrimitiveId::Ton | PrimitiveId::Tof | PrimitiveId::Tp) && i == 1;
            let v = VmValue::from_stack_value(out.values[i], prefer_time);
            self.push(v, pc)?;
        }
        Ok(())
    }

    fn push(&mut self, v: VmValue, pc: usize) -> Result<(), VmError> {
        if self.sp >= MAX_STACK {
            return Err(VmError::StackOverflow { pc });
        }
        self.stack[self.sp] = v;
        self.sp += 1;
        Ok(())
    }

    fn pop(&mut self, pc: usize) -> Result<VmValue, VmError> {
        if self.sp == 0 {
            return Err(VmError::StackUnderflow { pc });
        }
        self.sp -= 1;
        Ok(self.stack[self.sp])
    }

    fn binop(&mut self, op: Opcode, a: VmValue, b: VmValue, pc: usize) -> Result<VmValue, VmError> {
        match op {
            Opcode::And | Opcode::Or | Opcode::Xor => logic_op(op, a, b, pc),
            Opcode::Eq => Ok(VmValue::Bool(values_eq(a, b))),
            Opcode::Ne => Ok(VmValue::Bool(!values_eq(a, b))),
            Opcode::Lt | Opcode::Le | Opcode::Gt | Opcode::Ge => cmp_op(op, a, b, pc),
            Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div => {
                arith_op(op, a, b, pc, &mut self.div0_count)
            }
            _ => Err(VmError::Type {
                pc,
                detail: format!("not a binary op {op:?}"),
            }),
        }
    }
}

fn unary_op(op: Opcode, value: VmValue, pc: usize) -> Result<VmValue, VmError> {
    match op {
        Opcode::Not => match value {
            VmValue::Bool(b) => Ok(VmValue::Bool(!b)),
            VmValue::Int(v) => Ok(VmValue::Int(!v)),
            VmValue::Dint(v) => Ok(VmValue::Dint(!v)),
            VmValue::Time(v) => Ok(VmValue::Time(!v)),
            VmValue::Lint(v) => Ok(VmValue::Lint(!v)),
            VmValue::Real(_) => Err(VmError::Type {
                pc,
                detail: "NOT on REAL".into(),
            }),
        },
        Opcode::Neg => match value {
            VmValue::Int(v) => Ok(VmValue::Int(v.wrapping_neg())),
            VmValue::Dint(v) => Ok(VmValue::Dint(v.wrapping_neg())),
            VmValue::Time(v) => Ok(VmValue::Time(v.wrapping_neg())),
            VmValue::Lint(v) => Ok(VmValue::Lint(v.wrapping_neg())),
            VmValue::Real(v) => Ok(VmValue::Real(-v)),
            VmValue::Bool(_) => Err(VmError::Type {
                pc,
                detail: "NEG on BOOL".into(),
            }),
        },
        _ => Err(VmError::Type {
            pc,
            detail: format!("not a unary op {op:?}"),
        }),
    }
}

fn parse_user_fb_id(name: &str) -> Option<u32> {
    // fb.RS → None (use sequential); fb.1 / user.3 → Some
    let rest = name
        .strip_prefix("fb.")
        .or_else(|| name.strip_prefix("FB."))?;
    rest.parse().ok()
}

fn values_eq(a: VmValue, b: VmValue) -> bool {
    match (a, b) {
        (VmValue::Bool(x), VmValue::Bool(y)) => x == y,
        (VmValue::Real(x), VmValue::Real(y)) => x.to_bits() == y.to_bits(),
        (VmValue::Real(_), _) | (_, VmValue::Real(_)) => false,
        _ => a.as_i64().zip(b.as_i64()).is_some_and(|(x, y)| x == y),
    }
}

fn logic_op(op: Opcode, a: VmValue, b: VmValue, pc: usize) -> Result<VmValue, VmError> {
    match (a, b) {
        (VmValue::Bool(x), VmValue::Bool(y)) => Ok(VmValue::Bool(match op {
            Opcode::And => x && y,
            Opcode::Or => x || y,
            Opcode::Xor => x ^ y,
            _ => unreachable!(),
        })),
        (VmValue::Dint(x), VmValue::Dint(y)) => Ok(VmValue::Dint(match op {
            Opcode::And => x & y,
            Opcode::Or => x | y,
            Opcode::Xor => x ^ y,
            _ => unreachable!(),
        })),
        (VmValue::Int(x), VmValue::Int(y)) => Ok(VmValue::Int(match op {
            Opcode::And => x & y,
            Opcode::Or => x | y,
            Opcode::Xor => x ^ y,
            _ => unreachable!(),
        })),
        _ => Err(VmError::Type {
            pc,
            detail: format!("logic type mismatch {a:?} {b:?}"),
        }),
    }
}

fn cmp_op(op: Opcode, a: VmValue, b: VmValue, pc: usize) -> Result<VmValue, VmError> {
    let r = match (a, b) {
        (VmValue::Real(x), VmValue::Real(y)) => match op {
            Opcode::Lt => x < y,
            Opcode::Le => x <= y,
            Opcode::Gt => x > y,
            Opcode::Ge => x >= y,
            _ => false,
        },
        _ => {
            let x = a.as_i64().ok_or_else(|| VmError::Type {
                pc,
                detail: "cmp needs numeric".into(),
            })?;
            let y = b.as_i64().ok_or_else(|| VmError::Type {
                pc,
                detail: "cmp needs numeric".into(),
            })?;
            match op {
                Opcode::Lt => x < y,
                Opcode::Le => x <= y,
                Opcode::Gt => x > y,
                Opcode::Ge => x >= y,
                _ => false,
            }
        }
    };
    Ok(VmValue::Bool(r))
}

fn arith_op(
    op: Opcode,
    a: VmValue,
    b: VmValue,
    pc: usize,
    div0: &mut u32,
) -> Result<VmValue, VmError> {
    match (a, b) {
        (VmValue::Real(x), VmValue::Real(y)) => {
            let r = match op {
                Opcode::Add => x + y,
                Opcode::Sub => x - y,
                Opcode::Mul => x * y,
                Opcode::Div => {
                    if y == 0.0 {
                        *div0 += 1;
                        0.0
                    } else {
                        x / y
                    }
                }
                _ => {
                    return Err(VmError::Type {
                        pc,
                        detail: "bad arith".into(),
                    });
                }
            };
            Ok(VmValue::Real(r))
        }
        (VmValue::Time(x), VmValue::Time(y)) => Ok(VmValue::Time(int_arith(op, x, y, div0))),
        (VmValue::Dint(x), VmValue::Dint(y)) => Ok(VmValue::Dint(int_arith(op, x, y, div0))),
        (VmValue::Int(x), VmValue::Int(y)) => {
            Ok(VmValue::Int(
                int_arith(op, i32::from(x), i32::from(y), div0) as i16,
            ))
        }
        (VmValue::Lint(x), VmValue::Lint(y)) => Ok(VmValue::Lint(match op {
            Opcode::Add => x.wrapping_add(y),
            Opcode::Sub => x.wrapping_sub(y),
            Opcode::Mul => x.wrapping_mul(y),
            Opcode::Div => {
                if y == 0 {
                    *div0 += 1;
                    0
                } else {
                    x / y
                }
            }
            _ => {
                return Err(VmError::Type {
                    pc,
                    detail: "bad arith".into(),
                });
            }
        })),
        (VmValue::Int(x), VmValue::Dint(y)) => {
            Ok(VmValue::Dint(int_arith(op, i32::from(x), y, div0)))
        }
        (VmValue::Dint(x), VmValue::Int(y)) => {
            Ok(VmValue::Dint(int_arith(op, x, i32::from(y), div0)))
        }
        _ => Err(VmError::Type {
            pc,
            detail: format!("arith type mismatch {a:?} {b:?}"),
        }),
    }
}

fn int_arith(op: Opcode, x: i32, y: i32, div0: &mut u32) -> i32 {
    match op {
        Opcode::Add => x.wrapping_add(y),
        Opcode::Sub => x.wrapping_sub(y),
        Opcode::Mul => x.wrapping_mul(y),
        Opcode::Div => {
            if y == 0 {
                *div0 += 1;
                0
            } else {
                x / y
            }
        }
        _ => 0,
    }
}
