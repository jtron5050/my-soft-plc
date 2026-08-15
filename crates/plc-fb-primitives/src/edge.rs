//! Rising and falling edge detectors.

/// Rising-edge detector (`R_TRIG`).
///
/// `Q` is true for one evaluation when `clk` transitions false→true.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RTrig {
    /// One-shot output.
    pub q: bool,
    /// Previous clock sample.
    pub prev: bool,
}

impl RTrig {
    /// Cold-init.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            q: false,
            prev: false,
        }
    }

    /// Evaluate. Returns `Q`.
    pub fn eval(&mut self, clk: bool) -> bool {
        self.q = clk && !self.prev;
        self.prev = clk;
        self.q
    }
}

/// Falling-edge detector (`F_TRIG`).
///
/// `Q` is true for one evaluation when `clk` transitions true→false.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FTrig {
    /// One-shot output.
    pub q: bool,
    /// Previous clock sample.
    pub prev: bool,
}

impl FTrig {
    /// Cold-init.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            q: false,
            prev: false,
        }
    }

    /// Evaluate. Returns `Q`.
    pub fn eval(&mut self, clk: bool) -> bool {
        self.q = !clk && self.prev;
        self.prev = clk;
        self.q
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r_trig_one_shot() {
        let mut e = RTrig::new();
        assert!(!e.eval(false));
        assert!(e.eval(true));
        assert!(!e.eval(true));
        assert!(!e.eval(false));
        assert!(e.eval(true));
    }

    #[test]
    fn f_trig_one_shot() {
        let mut e = FTrig::new();
        assert!(!e.eval(false));
        assert!(!e.eval(true));
        assert!(e.eval(false));
        assert!(!e.eval(false));
    }
}
