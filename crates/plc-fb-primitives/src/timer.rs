//! On-delay, off-delay, and pulse timers (monotonic milliseconds).

/// Clamp elapsed time to `i32` TIME range (~24.8 days).
fn et_from_delta(start_ms: u64, now_ms: u64, pt: i32) -> i32 {
    let pt = pt.max(0);
    let delta = now_ms.saturating_sub(start_ms);
    let et = i32::try_from(delta.min(i32::MAX as u64)).unwrap_or(i32::MAX);
    et.min(pt)
}

/// IEC-style TON (on-delay timer).
///
/// `Q` becomes true when `IN` has been continuously true for at least `PT` ms
/// of monotonic time. `ET` tracks elapsed time while timing (capped at `PT`).
///
/// Instance layout matches architecture Appendix A.2 (32-byte aligned cell).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Ton {
    /// Output: timer elapsed.
    pub q: bool,
    /// Elapsed time (ms), 0..PT while timing.
    pub et: i32,
    /// Monotonic timestamp when IN last rose (or timing restarted).
    pub start_ms: u64,
    /// True while IN is true and timing (or done).
    pub running: bool,
}

impl Ton {
    /// Cold-init (activate / power-cycle non-retain).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            q: false,
            et: 0,
            start_ms: 0,
            running: false,
        }
    }

    /// Evaluate one sample.
    ///
    /// Returns `(Q, ET)`.
    pub fn eval(&mut self, r#in: bool, pt: i32, now_ms: u64) -> (bool, i32) {
        let pt = pt.max(0);
        if r#in {
            if !self.running {
                self.running = true;
                self.start_ms = now_ms;
                self.et = 0;
                self.q = pt == 0;
            } else {
                self.et = et_from_delta(self.start_ms, now_ms, pt);
                self.q = self.et >= pt;
            }
        } else {
            self.running = false;
            self.q = false;
            self.et = 0;
            self.start_ms = 0;
        }
        (self.q, self.et)
    }
}

/// IEC-style TOF (off-delay timer).
///
/// `Q` is true while `IN` is true, and remains true for `PT` ms after `IN`
/// falls.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tof {
    /// Output.
    pub q: bool,
    /// Elapsed time since IN fell (while off-delaying).
    pub et: i32,
    /// When IN last fell (start of off-delay).
    pub fall_ms: u64,
    /// Off-delay in progress.
    pub delaying: bool,
}

impl Tof {
    /// Cold-init.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            q: false,
            et: 0,
            fall_ms: 0,
            delaying: false,
        }
    }

    /// Evaluate one sample. Returns `(Q, ET)`.
    pub fn eval(&mut self, r#in: bool, pt: i32, now_ms: u64) -> (bool, i32) {
        let pt = pt.max(0);
        if r#in {
            self.q = true;
            self.et = 0;
            self.delaying = false;
            self.fall_ms = 0;
        } else if self.q || self.delaying {
            if !self.delaying {
                self.delaying = true;
                self.fall_ms = now_ms;
                self.et = 0;
            }
            self.et = et_from_delta(self.fall_ms, now_ms, pt);
            if self.et >= pt {
                self.q = false;
                self.delaying = false;
                self.et = pt;
            } else {
                self.q = true;
            }
        } else {
            self.q = false;
            self.et = 0;
        }
        (self.q, self.et)
    }
}

/// IEC-style TP (pulse timer).
///
/// On rising edge of `IN`, `Q` is true for `PT` ms regardless of further IN
/// changes until the pulse completes.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tp {
    /// Pulse output.
    pub q: bool,
    /// Elapsed time of the current pulse.
    pub et: i32,
    /// Pulse start monotonic time.
    pub start_ms: u64,
    /// Pulse active.
    pub running: bool,
    /// Previous IN for edge detect.
    pub prev_in: bool,
}

impl Tp {
    /// Cold-init.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            q: false,
            et: 0,
            start_ms: 0,
            running: false,
            prev_in: false,
        }
    }

    /// Evaluate one sample. Returns `(Q, ET)`.
    pub fn eval(&mut self, r#in: bool, pt: i32, now_ms: u64) -> (bool, i32) {
        let pt = pt.max(0);
        let rising = r#in && !self.prev_in;
        self.prev_in = r#in;

        if !self.running && rising {
            self.running = true;
            self.start_ms = now_ms;
            self.et = 0;
            if pt == 0 {
                // Zero-length pulse: Q true for this sample only, then done.
                self.q = true;
                self.running = false;
                self.et = 0;
                return (self.q, self.et);
            }
            self.q = true;
        }

        if self.running {
            self.et = et_from_delta(self.start_ms, now_ms, pt);
            if self.et >= pt {
                self.q = false;
                self.running = false;
                self.et = pt;
            } else {
                self.q = true;
            }
        } else {
            self.q = false;
            self.et = 0;
        }
        (self.q, self.et)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ton_delays_then_sets_q() {
        let mut t = Ton::new();
        assert_eq!(t.eval(true, 100, 1000), (false, 0));
        assert_eq!(t.eval(true, 100, 1050), (false, 50));
        assert_eq!(t.eval(true, 100, 1100), (true, 100));
        assert_eq!(t.eval(true, 100, 1200), (true, 100));
        assert_eq!(t.eval(false, 100, 1300), (false, 0));
    }

    #[test]
    fn ton_restarts_on_in_reassert() {
        let mut t = Ton::new();
        t.eval(true, 100, 0);
        t.eval(true, 100, 50);
        t.eval(false, 100, 60);
        assert_eq!(t.eval(true, 100, 60), (false, 0));
        assert_eq!(t.eval(true, 100, 160), (true, 100));
    }

    #[test]
    fn ton_uses_real_monotonic_gap() {
        // Overrun / pause: jump from 0 to 5000 with PT=100 → expires correctly.
        let mut t = Ton::new();
        t.eval(true, 100, 0);
        assert_eq!(t.eval(true, 100, 5000), (true, 100));
    }

    #[test]
    fn tof_holds_after_in_falls() {
        let mut t = Tof::new();
        assert_eq!(t.eval(true, 100, 0), (true, 0));
        assert_eq!(t.eval(false, 100, 0), (true, 0));
        assert_eq!(t.eval(false, 100, 50), (true, 50));
        assert_eq!(t.eval(false, 100, 100), (false, 100));
    }

    #[test]
    fn tp_pulse_on_rising_edge() {
        let mut t = Tp::new();
        assert_eq!(t.eval(false, 100, 0), (false, 0));
        assert_eq!(t.eval(true, 100, 0), (true, 0));
        // IN drops mid-pulse — Q continues.
        assert_eq!(t.eval(false, 100, 40), (true, 40));
        assert_eq!(t.eval(false, 100, 100), (false, 100));
        // No re-trigger without rising edge after complete.
        assert_eq!(t.eval(false, 100, 200), (false, 0));
        assert_eq!(t.eval(true, 100, 200), (true, 0));
    }
}
