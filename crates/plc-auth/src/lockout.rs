//! Per-IP sliding-window lockout.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::time::{Duration, Instant};

/// Failure window is one minute (architecture: 5 failures/min/IP).
const FAIL_WINDOW: Duration = Duration::from_secs(60);

/// Bound on tracked source IPs (management path; attacker-controlled keys).
const MAX_TRACKED_IPS: usize = 4096;

struct IpState {
    failures: VecDeque<Instant>,
    locked_until: Option<Instant>,
}

impl IpState {
    fn new() -> Self {
        Self {
            failures: VecDeque::new(),
            locked_until: None,
        }
    }

    fn locked(&self, now: Instant) -> bool {
        self.locked_until.is_some_and(|until| now < until)
    }

    fn age_failures(&mut self, now: Instant) {
        while self
            .failures
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= FAIL_WINDOW)
        {
            self.failures.pop_front();
        }
    }

    fn idle(&self, now: Instant) -> bool {
        !self.locked(now) && self.failures.is_empty()
    }
}

/// Tracks auth failures and lockouts per source IP.
pub(crate) struct LockoutTracker {
    fail_per_min: u32,
    lockout: Duration,
    max_ips: usize,
    ips: HashMap<IpAddr, IpState>,
}

impl LockoutTracker {
    pub(crate) fn new(fail_per_min: u32, lockout_secs: u64) -> Self {
        Self::with_capacity(fail_per_min, lockout_secs, MAX_TRACKED_IPS)
    }

    fn with_capacity(fail_per_min: u32, lockout_secs: u64, max_ips: usize) -> Self {
        Self {
            fail_per_min: fail_per_min.max(1),
            lockout: Duration::from_secs(lockout_secs.max(1)),
            max_ips: max_ips.max(1),
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
        self.sweep(now);
        if !self.ips.contains_key(&ip) {
            self.evict_for_capacity(now);
        }
        let state = self.ips.entry(ip).or_insert_with(IpState::new);
        state.age_failures(now);
        state.failures.push_back(now);
        if state.failures.len() as u32 >= self.fail_per_min {
            let until = now + self.lockout;
            state.locked_until = Some(until);
            // Cooldown starts a fresh 5-failure budget; do not one-strike re-arm.
            state.failures.clear();
            Some(remaining_secs(until, now))
        } else {
            None
        }
    }

    fn sweep(&mut self, now: Instant) {
        self.ips.retain(|_, state| {
            if state.locked_until.is_some_and(|until| now >= until) {
                state.locked_until = None;
            }
            state.age_failures(now);
            !state.idle(now)
        });
    }

    fn evict_for_capacity(&mut self, now: Instant) {
        while self.ips.len() >= self.max_ips {
            let victim = self.oldest_unlocked(now).or_else(|| self.soonest_lock(now));
            match victim {
                Some(ip) => {
                    self.ips.remove(&ip);
                }
                None => break,
            }
        }
    }

    fn oldest_unlocked(&self, now: Instant) -> Option<IpAddr> {
        self.ips
            .iter()
            .filter(|(_, state)| !state.locked(now))
            .min_by_key(|(_, state)| state.failures.front().copied().unwrap_or(now))
            .map(|(ip, _)| *ip)
    }

    fn soonest_lock(&self, now: Instant) -> Option<IpAddr> {
        self.ips
            .iter()
            .min_by_key(|(_, state)| state.locked_until.unwrap_or(now))
            .map(|(ip, _)| *ip)
    }
}

pub(crate) fn remaining_secs(until: Instant, now: Instant) -> u64 {
    until.saturating_duration_since(now).as_secs().max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn ip(n: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(10, 0, 0, n))
    }

    #[test]
    fn lock_clears_failure_window() {
        let mut t = LockoutTracker::new(5, 60);
        let t0 = Instant::now();
        for i in 0..4 {
            assert_eq!(t.record_failure(ip(1), t0), None, "failure {i}");
        }
        assert!(t.record_failure(ip(1), t0).is_some());

        let after = t0 + Duration::from_secs(60);
        assert_eq!(t.locked_remaining(ip(1), after), None);
        for i in 0..4 {
            assert_eq!(
                t.record_failure(ip(1), after),
                None,
                "post-expiry failure {i}"
            );
        }
        assert!(t.record_failure(ip(1), after).is_some());
    }

    #[test]
    fn ages_failures_at_window_boundary() {
        let mut t = LockoutTracker::new(5, 60);
        let t0 = Instant::now();
        for _ in 0..4 {
            assert_eq!(t.record_failure(ip(1), t0), None);
        }
        let later = t0 + FAIL_WINDOW;
        assert_eq!(t.record_failure(ip(1), later), None);
        assert_eq!(t.ips.get(&ip(1)).unwrap().failures.len(), 1);
    }

    #[test]
    fn evicts_expired_and_caps_map() {
        let mut t = LockoutTracker::with_capacity(5, 60, 4);
        let t0 = Instant::now();
        for n in 1..=4 {
            t.record_failure(ip(n), t0);
        }
        assert_eq!(t.ips.len(), 4);
        t.record_failure(ip(5), t0);
        assert!(t.ips.len() <= 4);
        assert!(t.ips.contains_key(&ip(5)));

        let later = t0 + FAIL_WINDOW;
        t.record_failure(ip(6), later);
        assert!(t.ips.contains_key(&ip(6)));
        assert!(!t.ips.contains_key(&ip(1)));
        assert_eq!(t.ips.len(), 1);
    }
}
