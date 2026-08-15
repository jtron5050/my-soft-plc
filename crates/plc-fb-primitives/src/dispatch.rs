//! VM-facing primitive dispatch over a typed instance store.

use plc_ir::PrimitiveId;

use crate::counter::{Ctd, Ctu};
use crate::edge::{FTrig, RTrig};
use crate::latch::{Rs, Sr};
use crate::pid::Pid;
use crate::timer::{Tof, Ton, Tp};

/// Errors from [`call_primitive`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveCallError {
    /// Instance index out of range for the primitive kind.
    BadInstance,
    /// Wrong number of stack inputs for the primitive.
    Arity {
        /// Expected input count.
        expected: u8,
        /// Provided input count.
        got: u8,
    },
    /// Input value tag/type mismatch.
    TypeMismatch,
}

/// Stack scalar values passed into / out of a primitive call.
///
/// Kept independent of the full IR value representation so this crate stays
/// free of process-image coupling; the VM maps to/from IR tags.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StackValue {
    /// BOOL.
    Bool(bool),
    /// DINT / TIME (ms for TIME).
    Dint(i32),
    /// REAL.
    Real(f32),
}

impl StackValue {
    /// Interpret as BOOL.
    #[must_use]
    pub const fn as_bool(self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(b),
            _ => None,
        }
    }

    /// Interpret as i32 (DINT/TIME).
    #[must_use]
    pub const fn as_dint(self) -> Option<i32> {
        match self {
            Self::Dint(v) => Some(v),
            Self::Bool(b) => Some(if b { 1 } else { 0 }),
            Self::Real(_) => None,
        }
    }

    /// Interpret as f32.
    #[must_use]
    pub fn as_real(self) -> Option<f32> {
        match self {
            Self::Real(v) => Some(v),
            Self::Dint(v) => Some(v as f32),
            Self::Bool(b) => Some(if b { 1.0 } else { 0.0 }),
        }
    }
}

/// Outputs pushed back onto the VM stack (declaration order).
#[derive(Debug, Clone, PartialEq)]
pub struct PrimitiveOutputs {
    /// Output values in FB declaration order.
    pub values: [StackValue; 4],
    /// Number of valid outputs in `values`.
    pub count: u8,
}

impl PrimitiveOutputs {
    fn one(v: StackValue) -> Self {
        Self {
            values: [
                v,
                StackValue::Bool(false),
                StackValue::Bool(false),
                StackValue::Bool(false),
            ],
            count: 1,
        }
    }

    fn two(a: StackValue, b: StackValue) -> Self {
        Self {
            values: [a, b, StackValue::Bool(false), StackValue::Bool(false)],
            count: 2,
        }
    }
}

/// In-memory store of all primitive instances for one program image.
///
/// Allocated at arm time; never resized during RUN (RT rule).
#[derive(Debug, Clone)]
pub struct PrimitiveStore {
    /// TON instances.
    pub ton: Vec<Ton>,
    /// TOF instances.
    pub tof: Vec<Tof>,
    /// TP instances.
    pub tp: Vec<Tp>,
    /// CTU instances.
    pub ctu: Vec<Ctu>,
    /// CTD instances.
    pub ctd: Vec<Ctd>,
    /// RS instances.
    pub rs: Vec<Rs>,
    /// SR instances.
    pub sr: Vec<Sr>,
    /// R_TRIG instances.
    pub r_trig: Vec<RTrig>,
    /// F_TRIG instances.
    pub f_trig: Vec<FTrig>,
    /// PID instances.
    pub pid: Vec<Pid>,
}

impl PrimitiveStore {
    /// Empty store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            ton: Vec::new(),
            tof: Vec::new(),
            tp: Vec::new(),
            ctu: Vec::new(),
            ctd: Vec::new(),
            rs: Vec::new(),
            sr: Vec::new(),
            r_trig: Vec::new(),
            f_trig: Vec::new(),
            pid: Vec::new(),
        }
    }

    /// Pre-size all instance pools (arm-time allocation).
    pub fn with_capacity(n: usize) -> Self {
        Self {
            ton: vec![Ton::new(); n],
            tof: vec![Tof::new(); n],
            tp: vec![Tp::new(); n],
            ctu: vec![Ctu::new(); n],
            ctd: vec![Ctd::new(); n],
            rs: vec![Rs::new(); n],
            sr: vec![Sr::new(); n],
            r_trig: vec![RTrig::new(); n],
            f_trig: vec![FTrig::new(); n],
            pid: vec![Pid::default(); n],
        }
    }

    /// Cold-reset all instances (activate / non-retain policy).
    pub fn cold_reset_all(&mut self) {
        for x in &mut self.ton {
            *x = Ton::new();
        }
        for x in &mut self.tof {
            *x = Tof::new();
        }
        for x in &mut self.tp {
            *x = Tp::new();
        }
        for x in &mut self.ctu {
            *x = Ctu::new();
        }
        for x in &mut self.ctd {
            *x = Ctd::new();
        }
        for x in &mut self.rs {
            *x = Rs::new();
        }
        for x in &mut self.sr {
            *x = Sr::new();
        }
        for x in &mut self.r_trig {
            *x = RTrig::new();
        }
        for x in &mut self.f_trig {
            *x = FTrig::new();
        }
        for x in &mut self.pid {
            x.cold_reset();
        }
    }
}

impl Default for PrimitiveStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Invoke a primitive by id.
///
/// `inputs` are in declaration order (first → last). `instance` indexes the
/// corresponding pool in `store`. `now_ms` is the task-invocation monotonic
/// sample (ignored by non-timer/PID blocks).
///
/// # CALL_FB arity
///
/// Matches [`PrimitiveId::input_count`]:
/// - TON/TOF/TP: IN, PT
/// - CTU: CU, R, PV
/// - CTD: CD, LD, PV
/// - RS/SR: S, R
/// - R_TRIG/F_TRIG: CLK
/// - PID: PV, SP, enable
pub fn call_primitive(
    store: &mut PrimitiveStore,
    id: PrimitiveId,
    instance: usize,
    inputs: &[StackValue],
    now_ms: u64,
) -> Result<PrimitiveOutputs, PrimitiveCallError> {
    let expected = id.input_count();
    if inputs.len() != expected as usize {
        return Err(PrimitiveCallError::Arity {
            expected,
            got: inputs.len() as u8,
        });
    }

    match id {
        PrimitiveId::Ton => {
            let inst = store
                .ton
                .get_mut(instance)
                .ok_or(PrimitiveCallError::BadInstance)?;
            let r#in = inputs[0]
                .as_bool()
                .ok_or(PrimitiveCallError::TypeMismatch)?;
            let pt = inputs[1]
                .as_dint()
                .ok_or(PrimitiveCallError::TypeMismatch)?;
            let (q, et) = inst.eval(r#in, pt, now_ms);
            Ok(PrimitiveOutputs::two(
                StackValue::Bool(q),
                StackValue::Dint(et),
            ))
        }
        PrimitiveId::Tof => {
            let inst = store
                .tof
                .get_mut(instance)
                .ok_or(PrimitiveCallError::BadInstance)?;
            let r#in = inputs[0]
                .as_bool()
                .ok_or(PrimitiveCallError::TypeMismatch)?;
            let pt = inputs[1]
                .as_dint()
                .ok_or(PrimitiveCallError::TypeMismatch)?;
            let (q, et) = inst.eval(r#in, pt, now_ms);
            Ok(PrimitiveOutputs::two(
                StackValue::Bool(q),
                StackValue::Dint(et),
            ))
        }
        PrimitiveId::Tp => {
            let inst = store
                .tp
                .get_mut(instance)
                .ok_or(PrimitiveCallError::BadInstance)?;
            let r#in = inputs[0]
                .as_bool()
                .ok_or(PrimitiveCallError::TypeMismatch)?;
            let pt = inputs[1]
                .as_dint()
                .ok_or(PrimitiveCallError::TypeMismatch)?;
            let (q, et) = inst.eval(r#in, pt, now_ms);
            Ok(PrimitiveOutputs::two(
                StackValue::Bool(q),
                StackValue::Dint(et),
            ))
        }
        PrimitiveId::Ctu => {
            let inst = store
                .ctu
                .get_mut(instance)
                .ok_or(PrimitiveCallError::BadInstance)?;
            let cu = inputs[0]
                .as_bool()
                .ok_or(PrimitiveCallError::TypeMismatch)?;
            let r = inputs[1]
                .as_bool()
                .ok_or(PrimitiveCallError::TypeMismatch)?;
            let pv = inputs[2]
                .as_dint()
                .ok_or(PrimitiveCallError::TypeMismatch)?;
            let (q, cv) = inst.eval(cu, r, pv);
            Ok(PrimitiveOutputs::two(
                StackValue::Bool(q),
                StackValue::Dint(cv),
            ))
        }
        PrimitiveId::Ctd => {
            let inst = store
                .ctd
                .get_mut(instance)
                .ok_or(PrimitiveCallError::BadInstance)?;
            let cd = inputs[0]
                .as_bool()
                .ok_or(PrimitiveCallError::TypeMismatch)?;
            let ld = inputs[1]
                .as_bool()
                .ok_or(PrimitiveCallError::TypeMismatch)?;
            let pv = inputs[2]
                .as_dint()
                .ok_or(PrimitiveCallError::TypeMismatch)?;
            let (q, cv) = inst.eval(cd, ld, pv);
            Ok(PrimitiveOutputs::two(
                StackValue::Bool(q),
                StackValue::Dint(cv),
            ))
        }
        PrimitiveId::Rs => {
            let inst = store
                .rs
                .get_mut(instance)
                .ok_or(PrimitiveCallError::BadInstance)?;
            let s = inputs[0]
                .as_bool()
                .ok_or(PrimitiveCallError::TypeMismatch)?;
            let r = inputs[1]
                .as_bool()
                .ok_or(PrimitiveCallError::TypeMismatch)?;
            Ok(PrimitiveOutputs::one(StackValue::Bool(inst.eval(s, r))))
        }
        PrimitiveId::Sr => {
            let inst = store
                .sr
                .get_mut(instance)
                .ok_or(PrimitiveCallError::BadInstance)?;
            let s = inputs[0]
                .as_bool()
                .ok_or(PrimitiveCallError::TypeMismatch)?;
            let r = inputs[1]
                .as_bool()
                .ok_or(PrimitiveCallError::TypeMismatch)?;
            Ok(PrimitiveOutputs::one(StackValue::Bool(inst.eval(s, r))))
        }
        PrimitiveId::RTrig => {
            let inst = store
                .r_trig
                .get_mut(instance)
                .ok_or(PrimitiveCallError::BadInstance)?;
            let clk = inputs[0]
                .as_bool()
                .ok_or(PrimitiveCallError::TypeMismatch)?;
            Ok(PrimitiveOutputs::one(StackValue::Bool(inst.eval(clk))))
        }
        PrimitiveId::FTrig => {
            let inst = store
                .f_trig
                .get_mut(instance)
                .ok_or(PrimitiveCallError::BadInstance)?;
            let clk = inputs[0]
                .as_bool()
                .ok_or(PrimitiveCallError::TypeMismatch)?;
            Ok(PrimitiveOutputs::one(StackValue::Bool(inst.eval(clk))))
        }
        PrimitiveId::Pid => {
            let inst = store
                .pid
                .get_mut(instance)
                .ok_or(PrimitiveCallError::BadInstance)?;
            let pv = inputs[0]
                .as_real()
                .ok_or(PrimitiveCallError::TypeMismatch)?;
            let sp = inputs[1]
                .as_real()
                .ok_or(PrimitiveCallError::TypeMismatch)?;
            let en = inputs[2]
                .as_bool()
                .ok_or(PrimitiveCallError::TypeMismatch)?;
            let out = inst.eval(pv, sp, en, now_ms);
            Ok(PrimitiveOutputs::one(StackValue::Real(out)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_ton() {
        let mut store = PrimitiveStore::with_capacity(1);
        let out = call_primitive(
            &mut store,
            PrimitiveId::Ton,
            0,
            &[StackValue::Bool(true), StackValue::Dint(100)],
            0,
        )
        .unwrap();
        assert_eq!(out.count, 2);
        assert_eq!(out.values[0], StackValue::Bool(false));
        let out = call_primitive(
            &mut store,
            PrimitiveId::Ton,
            0,
            &[StackValue::Bool(true), StackValue::Dint(100)],
            100,
        )
        .unwrap();
        assert_eq!(out.values[0], StackValue::Bool(true));
    }

    #[test]
    fn dispatch_rs() {
        let mut store = PrimitiveStore::with_capacity(1);
        let out = call_primitive(
            &mut store,
            PrimitiveId::Rs,
            0,
            &[StackValue::Bool(true), StackValue::Bool(false)],
            0,
        )
        .unwrap();
        assert_eq!(out.values[0], StackValue::Bool(true));
        let out = call_primitive(
            &mut store,
            PrimitiveId::Rs,
            0,
            &[StackValue::Bool(false), StackValue::Bool(true)],
            0,
        )
        .unwrap();
        assert_eq!(out.values[0], StackValue::Bool(false));
    }

    #[test]
    fn arity_error() {
        let mut store = PrimitiveStore::with_capacity(1);
        let err = call_primitive(&mut store, PrimitiveId::Ton, 0, &[], 0).unwrap_err();
        assert_eq!(
            err,
            PrimitiveCallError::Arity {
                expected: 2,
                got: 0
            }
        );
    }
}
