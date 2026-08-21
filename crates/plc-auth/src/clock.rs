//! Injectable clock for lockout / rate-limit tests.

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Time source used by [`crate::AuthService`].
pub trait Clock: Send + Sync {
    /// Monotonic instant for lockout windows and token-bucket refill.
    fn now(&self) -> Instant;
    /// Wall-clock unix seconds for audit stamps (not used for TON/TOF).
    fn unix_secs(&self) -> u64;
}

/// Production clock: `Instant::now` and `SystemTime`.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn unix_secs(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

/// Test clock: start at construction, advance with [`FakeClock::advance`].
#[derive(Debug)]
pub struct FakeClock {
    base: Instant,
    offset: Mutex<Duration>,
    unix: Mutex<u64>,
}

impl FakeClock {
    /// Capture `Instant::now` as the origin.
    #[must_use]
    pub fn new() -> Self {
        Self {
            base: Instant::now(),
            offset: Mutex::new(Duration::ZERO),
            unix: Mutex::new(1_700_000_000),
        }
    }

    /// Advance monotonic and unix time by `d`.
    pub fn advance(&self, d: Duration) {
        *self.offset.lock().expect("fake clock offset") += d;
        *self.unix.lock().expect("fake clock unix") += d.as_secs();
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Instant {
        self.base + *self.offset.lock().expect("fake clock offset")
    }

    fn unix_secs(&self) -> u64 {
        *self.unix.lock().expect("fake clock unix")
    }
}
