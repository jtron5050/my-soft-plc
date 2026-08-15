//! Scale / offset / clamp for analog bindings.

/// Apply `eng = raw * scale + offset`, then optional clamp `[lo, hi]`.
#[must_use]
pub fn apply_scale_offset_clamp(raw: f64, scale: f64, offset: f64, clamp: Option<[f64; 2]>) -> f64 {
    let mut eng = raw * scale + offset;
    if let Some([lo, hi]) = clamp {
        let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        if eng < lo {
            eng = lo;
        } else if eng > hi {
            eng = hi;
        }
    }
    eng
}

/// Inverse for output scaling when drivers need raw register values.
#[must_use]
pub fn eng_to_raw(eng: f64, scale: f64, offset: f64) -> f64 {
    if scale == 0.0 {
        return 0.0;
    }
    (eng - offset) / scale
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_and_clamp() {
        let v = apply_scale_offset_clamp(675.0, 0.1, 0.0, Some([0.0, 100.0]));
        assert!((v - 67.5).abs() < f64::EPSILON);
        let hi = apply_scale_offset_clamp(2000.0, 0.1, 0.0, Some([0.0, 100.0]));
        assert!((hi - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn inverse() {
        let raw = eng_to_raw(67.5, 0.1, 0.0);
        assert!((raw - 675.0).abs() < f64::EPSILON);
    }
}
