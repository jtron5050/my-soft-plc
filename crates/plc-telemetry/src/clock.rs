//! Wall-clock source for Sparkplug timestamps (KD-19).

use std::time::{SystemTime, UNIX_EPOCH};

/// Wall time + NTP sync flag. Never used for TON/TOF/TP.
pub trait WallClock: Send + Sync {
    /// Unix epoch milliseconds (system / NTP timebase).
    fn unix_ms(&self) -> u64;
    /// `true` when the kernel clock is NTP-disciplined (not `STA_UNSYNC`).
    fn is_synchronized(&self) -> bool;
}

/// Production clock: `SystemTime` + Linux `ntp_adjtime`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemWallClock;

impl WallClock for SystemWallClock {
    fn unix_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    fn is_synchronized(&self) -> bool {
        linux_clock_synchronized()
    }
}

/// Deterministic clock for tests.
#[derive(Debug, Clone, Copy)]
pub struct MockWallClock {
    unix_ms: u64,
    synced: bool,
}

impl MockWallClock {
    /// Fixed timestamp and sync flag.
    #[must_use]
    pub const fn new(unix_ms: u64, synced: bool) -> Self {
        Self { unix_ms, synced }
    }
}

impl WallClock for MockWallClock {
    fn unix_ms(&self) -> u64 {
        self.unix_ms
    }

    fn is_synchronized(&self) -> bool {
        self.synced
    }
}

#[cfg(target_os = "linux")]
fn linux_clock_synchronized() -> bool {
    // SAFETY: `ntp_adjtime` with a zeroed `timex` (modes = 0) is the
    // documented query form; it does not adjust the clock.
    #[allow(unsafe_code)]
    unsafe {
        let mut tx: libc::timex = std::mem::zeroed();
        let ret = libc::ntp_adjtime(&mut tx);
        ret >= 0 && ret != libc::TIME_ERROR && (tx.status & libc::STA_UNSYNC) == 0
    }
}

#[cfg(not(target_os = "linux"))]
fn linux_clock_synchronized() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_reports_configured_state() {
        let c = MockWallClock::new(1_700_000_000_000, false);
        assert_eq!(c.unix_ms(), 1_700_000_000_000);
        assert!(!c.is_synchronized());
    }
}
