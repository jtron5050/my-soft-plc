//! Raw register values ↔ engineering [`plc_io::PlcValue`].

use plc_io::{
    apply_scale_offset_clamp, eng_to_raw, BindingDirection, PlcValue, RawType, ValueType,
};

use crate::validate::PlannedPoint;

pub(crate) fn decode_number(raw: f64, point: &PlannedPoint) -> PlcValue {
    let eng = if point.direction == BindingDirection::Input {
        apply_scale_offset_clamp(raw, point.scale, point.offset, point.clamp)
    } else {
        raw
    };
    finish(eng, point.value_type)
}

pub(crate) fn decode_regs(regs: &[u16], point: &PlannedPoint) -> PlcValue {
    let raw = match point.raw {
        RawType::Bool => {
            let reg = regs.first().copied().unwrap_or(0);
            let bit = point.bit.unwrap_or(0);
            f64::from((reg >> bit) & 1)
        }
        RawType::Int => f64::from(regs.first().copied().unwrap_or(0) as i16),
        RawType::Uint => f64::from(regs.first().copied().unwrap_or(0)),
        RawType::Dint => {
            let hi = u32::from(regs.first().copied().unwrap_or(0));
            let lo = u32::from(regs.get(1).copied().unwrap_or(0));
            f64::from(((hi << 16) | lo) as i32)
        }
        RawType::Real => {
            let hi = u32::from(regs.first().copied().unwrap_or(0));
            let lo = u32::from(regs.get(1).copied().unwrap_or(0));
            f64::from(f32::from_bits((hi << 16) | lo))
        }
    };
    decode_number(raw, point)
}

pub(crate) fn coil_on(value: PlcValue, point: &PlannedPoint) -> bool {
    let raw = eng_to_raw(plc_eng(value), point.scale, point.offset);
    raw.is_finite() && raw != 0.0
}

pub(crate) fn value_to_words(value: PlcValue, point: &PlannedPoint) -> Vec<u16> {
    let raw = eng_to_raw(plc_eng(value), point.scale, point.offset);
    match point.raw {
        RawType::Bool => vec![u16::from(coil_on(value, point))],
        RawType::Int => vec![sat_i16(raw) as u16],
        RawType::Uint => vec![sat_u16(raw)],
        RawType::Dint => {
            let n = sat_i32(raw) as u32;
            vec![(n >> 16) as u16, n as u16]
        }
        RawType::Real => {
            let n = sat_f32(raw).to_bits();
            vec![(n >> 16) as u16, n as u16]
        }
    }
}

fn plc_eng(value: PlcValue) -> f64 {
    match value {
        PlcValue::Bool(b) => {
            if b {
                1.0
            } else {
                0.0
            }
        }
        PlcValue::Int(n) => f64::from(n),
        PlcValue::Dint(n) | PlcValue::Time(n) => f64::from(n),
        PlcValue::Real(n) => f64::from(n),
    }
}

fn finish(eng: f64, ty: ValueType) -> PlcValue {
    match ty {
        ValueType::Bool => PlcValue::Bool(eng.is_finite() && eng != 0.0),
        ValueType::Int => PlcValue::Int(sat_i16(eng)),
        ValueType::Dint => PlcValue::Dint(sat_i32(eng)),
        ValueType::Time => PlcValue::Time(sat_i32(eng)),
        ValueType::Real => PlcValue::Real(sat_f32(eng)),
    }
}

fn sat_i16(x: f64) -> i16 {
    if !x.is_finite() {
        return 0;
    }
    let x = x.round();
    if x > f64::from(i16::MAX) {
        i16::MAX
    } else if x < f64::from(i16::MIN) {
        i16::MIN
    } else {
        x as i16
    }
}

fn sat_u16(x: f64) -> u16 {
    if !x.is_finite() {
        return 0;
    }
    let x = x.round();
    if x > f64::from(u16::MAX) {
        u16::MAX
    } else if x < 0.0 {
        0
    } else {
        x as u16
    }
}

fn sat_i32(x: f64) -> i32 {
    if !x.is_finite() {
        return 0;
    }
    let x = x.round();
    if x > f64::from(i32::MAX) {
        i32::MAX
    } else if x < f64::from(i32::MIN) {
        i32::MIN
    } else {
        x as i32
    }
}

fn sat_f32(x: f64) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    let max = f64::from(f32::MAX);
    if x > max {
        f32::MAX
    } else if x < -max {
        f32::MIN
    } else {
        x as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plc_io::RegisterType;

    fn point(
        raw: RawType,
        value_type: ValueType,
        scale: f64,
        clamp: Option<[f64; 2]>,
    ) -> PlannedPoint {
        PlannedPoint {
            tag: "t".into(),
            slot: 0,
            direction: BindingDirection::Input,
            table: RegisterType::Holding,
            pdu: 0,
            words: 1,
            raw,
            bit: None,
            value_type,
            scale,
            offset: 0.0,
            clamp,
            safe: PlcValue::Bool(false),
            input_pos: Some(0),
            output_pos: None,
        }
    }

    #[test]
    fn scales_int_into_real_and_clamps() {
        let p = point(RawType::Int, ValueType::Real, 0.1, Some([0.0, 100.0]));
        let v = decode_regs(&[1000], &p);
        assert_eq!(v, PlcValue::Real(100.0));
        let mid = decode_regs(&[675], &p);
        assert_eq!(mid, PlcValue::Real(67.5));
    }

    #[test]
    fn real_is_big_endian() {
        let mut p = point(RawType::Real, ValueType::Real, 1.0, None);
        p.words = 2;
        let bits = 12.5f32.to_bits();
        let regs = [(bits >> 16) as u16, bits as u16];
        assert_eq!(decode_regs(&regs, &p), PlcValue::Real(12.5));
    }

    #[test]
    fn output_inverse_saturates() {
        let mut p = point(RawType::Int, ValueType::Real, 0.1, None);
        p.direction = BindingDirection::Output;
        assert_eq!(value_to_words(PlcValue::Real(67.5), &p), vec![675]);
        assert_eq!(
            value_to_words(PlcValue::Real(1.0e9), &p),
            vec![i16::MAX as u16]
        );
    }

    #[test]
    fn coil_safe_off() {
        let mut p = point(RawType::Bool, ValueType::Bool, 1.0, None);
        p.direction = BindingDirection::Output;
        p.table = RegisterType::Coil;
        assert!(!coil_on(PlcValue::Bool(false), &p));
        assert!(coil_on(PlcValue::Bool(true), &p));
    }
}
