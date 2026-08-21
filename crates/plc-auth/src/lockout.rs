//! Per-IP sliding-window lockout.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::time::{Duration, Instant};

/// Failure window is one minute (architecture: 5 failures/min/IP).
const FAIL_WINDOW: Duration = Duration::from_secs(60);

#[derive(Default)]
struct IpState {
    failures: VecDeque<Instant>,
    locked_until: Option<Instant>,
}

/// Tracks auth failures and lockouts per source IP.
pub(crate) struct LockoutTracker {
    fail_per_min: u32,
    lockout: Duration,
    ips: HashMap<IpAddr, IpState>,
}

impl LockoutTracker {
    pub(crate) fn new(fail_per_min: u32, lockout_secs: u64) -> Self {
        Self {
            fail_per_min: fail_per_min.max(1),
            lockout: Duration::from_secs(lockout_secs.max(1)),
            ips: HashMap::new(),
        }
    }

    /// Remaining lockout seconds, or `None` if the IP is not locked.
    pub(crate) fn locked_remaining(&mut self, ip: IpAddr, now: Instant) -> Option<u64> {
        let state = self.ips.get_mut(&ip)?;
        let until = state.locked_until?;
        if now < until {
            Some(remaining_secs(until, now))
        } else {
            state.locked_until = None;
            None
        }
    }

    /// Record a failure. Returns `Some(retry_after_secs)` if this failure
    /// crossed the threshold and a lockout is now active.
    pub(crate) fn record_failure(&mut self, ip: IpAddr, now: Instant) -> Option<u64> {
        if let Some(secs) = self.locked_remaining(ip, now) {
            return Some(secs);
        }
        let state = self.ips.entry(ip).or_default();
        while state
            .failures
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) > FAIL_WINDOW)
        {
            state.failures.pop_front();
        }
        state.failures.push_back(now);
        if state.failures.len() as u32 >= self.fail_per_min {
            let until = now + self.lockout;
            state.locked_until = Some(until);
            Some(remaining_secs(until, now))
        } else {
            None
        }
    }
}

pub(crate) fn remaining_secs(until: Instant, now: Instant) -> u64 {
    until.saturating_duration_since(now).as_secs().max(1)
}
