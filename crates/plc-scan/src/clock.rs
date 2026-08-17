//! Injectable clocks. Scan logic uses monotonic time only (KD-16).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Time source for the scan engine.
///
/// Unit tests inject [`VirtualClock`]. Production uses [`MonotonicClock`].
pub trait ScanClock: Send {
    /// Nanoseconds since this clock's origin.
    fn now_ns(&self) -> u64;

    /// Milliseconds since this clock's origin (truncated).
    fn now_ms(&self) -> u64 {
        self.now_ns() / 1_000_000
    }
}

/// `std::time::Instant` origin — never wall / NTP (KD-16).
#[derive(Debug)]
pub struct MonotonicClock {
    start: Instant,
}

impl MonotonicClock {
    /// Start counting from now.
    #[must_use]
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Default for MonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl ScanClock for MonotonicClock {
    fn now_ns(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

/// Deterministic clock shared between tests and the engine.
#[derive(Debug, Clone)]
pub struct VirtualClock {
    now_ns: Arc<AtomicU64>,
}

impl VirtualClock {
    /// Origin at 0 ns.
    #[must_use]
    pub fn new() -> Self {
        Self {
            now_ns: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Absolute time in milliseconds.
    pub fn set_ms(&self, ms: u64) {
        self.now_ns
            .store(ms.saturating_mul(1_000_000), Ordering::Release);
    }

    /// Advance by `ms` milliseconds.
    pub fn advance_ms(&self, ms: u64) {
        self.now_ns
            .fetch_add(ms.saturating_mul(1_000_000), Ordering::Release);
    }

    /// Advance by `ns` nanoseconds.
    pub fn advance_ns(&self, ns: u64) {
        self.now_ns.fetch_add(ns, Ordering::Release);
    }
}

impl Default for VirtualClock {
    fn default() -> Self {
        Self::new()
    }
}

impl ScanClock for VirtualClock {
    fn now_ns(&self) -> u64 {
        self.now_ns.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_advances() {
        let c = VirtualClock::new();
        assert_eq!(c.now_ms(), 0);
        c.advance_ms(50);
        assert_eq!(c.now_ms(), 50);
        c.set_ms(7);
        assert_eq!(c.now_ms(), 7);
    }

    #[test]
    fn monotonic_moves() {
        let c = MonotonicClock::new();
        let _ = c.now_ns();
    }
}
