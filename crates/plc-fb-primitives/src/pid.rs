//! Simple discrete PID controller for process loops (materials plants).

/// PID instance with gains and integrator state.
///
/// Discrete positional form using sample interval derived from monotonic
/// `now_ms` deltas. Gains live in the instance (not on the CALL_FB stack);
/// stack inputs are PV, SP, enable (matches `PrimitiveId::Pid` arity).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pid {
    /// Proportional gain.
    pub kp: f32,
    /// Integral gain (1/s units × error · dt).
    pub ki: f32,
    /// Derivative gain.
    pub kd: f32,
    /// Output low clamp.
    pub out_min: f32,
    /// Output high clamp.
    pub out_max: f32,
    /// Controller output.
    pub out: f32,
    /// Accumulated integral term.
    integral: f32,
    /// Previous error for D term.
    last_err: f32,
    /// Previous sample time.
    last_now_ms: u64,
    /// Whether `last_*` are valid.
    initialized: bool,
}

impl Default for Pid {
    fn default() -> Self {
        Self::new(1.0, 0.0, 0.0, -100.0, 100.0)
    }
}

impl Pid {
    /// Create with gains and output limits.
    #[must_use]
    pub const fn new(kp: f32, ki: f32, kd: f32, out_min: f32, out_max: f32) -> Self {
        Self {
            kp,
            ki,
            kd,
            out_min,
            out_max,
            out: 0.0,
            integral: 0.0,
            last_err: 0.0,
            last_now_ms: 0,
            initialized: false,
        }
    }

    /// Cold-init controller state (gains preserved).
    pub fn cold_reset(&mut self) {
        self.out = 0.0;
        self.integral = 0.0;
        self.last_err = 0.0;
        self.last_now_ms = 0;
        self.initialized = false;
    }

    /// Integrator accumulator (tests / hot-swap policy).
    #[must_use]
    pub const fn integral(&self) -> f32 {
        self.integral
    }

    /// Whether the controller has taken a sample since cold-init.
    #[must_use]
    pub const fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Evaluate one sample.
    ///
    /// - `pv`: process variable
    /// - `sp`: setpoint
    /// - `enable`: when false, holds last output and freezes integrator
    /// - `now_ms`: monotonic sample time (task invocation timebase)
    ///
    /// Returns controller output (clamped).
    pub fn eval(&mut self, pv: f32, sp: f32, enable: bool, now_ms: u64) -> f32 {
        if !enable {
            return self.out;
        }

        let err = sp - pv;
        let dt_s = if self.initialized {
            let dms = now_ms.saturating_sub(self.last_now_ms);
            // Guard absurd dt (first sample after long STOP is still OK via real delta).
            (dms as f32) / 1000.0
        } else {
            0.0
        };

        if self.initialized && dt_s > 0.0 {
            self.integral = (self.integral + err * dt_s).clamp(
                self.out_min / self.ki_or_one(),
                self.out_max / self.ki_or_one(),
            );
            // Soft anti-windup: clamp integral contribution roughly to output range.
            let i_term = self.ki * self.integral;
            let d_term = self.kd * (err - self.last_err) / dt_s;
            let p_term = self.kp * err;
            self.out = (p_term + i_term + d_term).clamp(self.out_min, self.out_max);
        } else {
            // First sample or zero dt: P-only.
            self.out = (self.kp * err).clamp(self.out_min, self.out_max);
            self.integral = 0.0;
        }

        self.last_err = err;
        self.last_now_ms = now_ms;
        self.initialized = true;
        self.out
    }

    fn ki_or_one(&self) -> f32 {
        if self.ki.abs() < f32::EPSILON {
            1.0
        } else {
            self.ki.abs()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pid_proportional_response() {
        let mut pid = Pid::new(2.0, 0.0, 0.0, -100.0, 100.0);
        let out = pid.eval(0.0, 10.0, true, 0);
        assert!((out - 20.0).abs() < 1e-4);
    }

    #[test]
    fn pid_integral_accumulates() {
        let mut pid = Pid::new(0.0, 1.0, 0.0, -100.0, 100.0);
        pid.eval(0.0, 1.0, true, 0);
        let out = pid.eval(0.0, 1.0, true, 1000); // 1 s later
        assert!(out > 0.5, "expected integral action, got {out}");
    }

    #[test]
    fn pid_disabled_holds() {
        let mut pid = Pid::new(1.0, 0.0, 0.0, -100.0, 100.0);
        let o1 = pid.eval(0.0, 5.0, true, 0);
        let o2 = pid.eval(0.0, 100.0, false, 100);
        assert!((o1 - o2).abs() < 1e-6);
    }

    #[test]
    fn pid_cold_reset_clears_state() {
        let mut pid = Pid::new(1.0, 1.0, 0.0, -10.0, 10.0);
        pid.eval(0.0, 1.0, true, 0);
        pid.eval(0.0, 1.0, true, 500);
        pid.cold_reset();
        assert!((pid.out).abs() < 1e-6);
        assert!((pid.integral()).abs() < 1e-6);
        assert!(!pid.is_initialized());
    }
}
