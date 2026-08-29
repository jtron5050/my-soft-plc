//! Sliding-window limit: 100 forced-tag operations per minute.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(60);
const MAX_OPS: usize = 100;

/// Per-process force-op limiter (architecture `GET /tags` fan-out note).
#[derive(Debug)]
pub struct ForceLimiter {
    hits: VecDeque<Instant>,
}

impl ForceLimiter {
    /// Empty window.
    #[must_use]
    pub fn new() -> Self {
        Self {
            hits: VecDeque::new(),
        }
    }

    /// Record one force. `Err(retry_after_secs)` when over cap.
    pub fn check(&mut self, now: Instant) -> Result<(), u64> {
        while let Some(front) = self.hits.front() {
            if now.saturating_duration_since(*front) >= WINDOW {
                self.hits.pop_front();
            } else {
                break;
            }
        }
        if self.hits.len() >= MAX_OPS {
            let wait = WINDOW
                .checked_sub(now.saturating_duration_since(self.hits[0]))
                .unwrap_or(Duration::from_secs(1));
            return Err(wait.as_secs().max(1));
        }
        self.hits.push_back(now);
        Ok(())
    }
}

impl Default for ForceLimiter {
    fn default() -> Self {
        Self::new()
    }
}
