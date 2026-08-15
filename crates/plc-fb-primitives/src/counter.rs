//! Count-up and count-down function blocks.

/// IEC-style CTU (count up).
///
/// Counts rising edges of `cu` into `cv` until reset by `r`. `q` is true when
/// `cv >= pv`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Ctu {
    /// Count reached preset.
    pub q: bool,
    /// Current count value.
    pub cv: i32,
    /// Previous CU for edge detection.
    pub prev_cu: bool,
}

impl Ctu {
    /// Cold-init.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            q: false,
            cv: 0,
            prev_cu: false,
        }
    }

    /// Evaluate. Returns `(Q, CV)`.
    pub fn eval(&mut self, cu: bool, r: bool, pv: i32) -> (bool, i32) {
        if r {
            self.cv = 0;
        } else {
            let rising = cu && !self.prev_cu;
            if rising && self.cv < i32::MAX {
                self.cv = self.cv.saturating_add(1);
            }
        }
        self.prev_cu = cu;
        self.q = self.cv >= pv;
        (self.q, self.cv)
    }
}

/// IEC-style CTD (count down).
///
/// Loads `pv` into `cv` on `ld` rising; counts down on `cd` rising. `q` is true
/// when `cv <= 0`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Ctd {
    /// Zero reached.
    pub q: bool,
    /// Current count value.
    pub cv: i32,
    /// Previous CD for edge detection.
    pub prev_cd: bool,
    /// Previous LD for edge detection.
    pub prev_ld: bool,
}

impl Ctd {
    /// Cold-init.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            q: false,
            cv: 0,
            prev_cd: false,
            prev_ld: false,
        }
    }

    /// Evaluate. Returns `(Q, CV)`.
    pub fn eval(&mut self, cd: bool, ld: bool, pv: i32) -> (bool, i32) {
        let load_edge = ld && !self.prev_ld;
        let count_edge = cd && !self.prev_cd;
        if load_edge {
            self.cv = pv;
        } else if count_edge && self.cv > i32::MIN {
            self.cv = self.cv.saturating_sub(1);
        }
        self.prev_cd = cd;
        self.prev_ld = ld;
        self.q = self.cv <= 0;
        (self.q, self.cv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctu_counts_rising_edges() {
        let mut c = Ctu::new();
        assert_eq!(c.eval(false, false, 3), (false, 0));
        assert_eq!(c.eval(true, false, 3), (false, 1));
        assert_eq!(c.eval(true, false, 3), (false, 1)); // level held: no extra count
        assert_eq!(c.eval(false, false, 3), (false, 1));
        assert_eq!(c.eval(true, false, 3), (false, 2));
        assert_eq!(c.eval(false, false, 3), (false, 2));
        assert_eq!(c.eval(true, false, 3), (true, 3));
        assert_eq!(c.eval(false, true, 3), (false, 0)); // reset
    }

    #[test]
    fn ctd_loads_and_counts_down() {
        let mut c = Ctd::new();
        assert_eq!(c.eval(false, true, 2), (false, 2));
        assert_eq!(c.eval(false, false, 2), (false, 2));
        assert_eq!(c.eval(true, false, 2), (false, 1));
        assert_eq!(c.eval(false, false, 2), (false, 1));
        assert_eq!(c.eval(true, false, 2), (true, 0));
    }
}
