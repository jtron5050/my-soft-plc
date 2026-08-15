//! RS (reset-dominant) and SR (set-dominant) latches.

/// Reset-dominant bistable: `Q := (S OR Q) AND NOT R`.
///
/// When both S and R are true, R wins (`Q = false`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rs {
    /// Latched output.
    pub q: bool,
}

impl Rs {
    /// Cold-init.
    #[must_use]
    pub const fn new() -> Self {
        Self { q: false }
    }

    /// Evaluate. Returns `Q`.
    pub fn eval(&mut self, s: bool, r: bool) -> bool {
        // Q := (S OR Q) AND NOT R
        self.q = (s || self.q) && !r;
        self.q
    }
}

/// Set-dominant bistable: `Q := S OR (Q AND NOT R)`.
///
/// When both S and R are true, S wins (`Q = true`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sr {
    /// Latched output.
    pub q: bool,
}

impl Sr {
    /// Cold-init.
    #[must_use]
    pub const fn new() -> Self {
        Self { q: false }
    }

    /// Evaluate. Returns `Q`.
    pub fn eval(&mut self, s: bool, r: bool) -> bool {
        self.q = s || (self.q && !r);
        self.q
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rs_reset_dominant() {
        let mut rs = Rs::new();
        assert!(!rs.eval(false, false));
        assert!(rs.eval(true, false));
        assert!(rs.eval(false, false)); // holds
        assert!(!rs.eval(false, true)); // reset
        assert!(!rs.eval(true, true)); // both: R wins
    }

    #[test]
    fn sr_set_dominant() {
        let mut sr = Sr::new();
        assert!(!sr.eval(false, false));
        assert!(sr.eval(true, false));
        assert!(sr.eval(false, false));
        assert!(!sr.eval(false, true));
        assert!(sr.eval(true, true)); // both: S wins
    }
}
