//! Per-principal token-bucket rate limiter.

use std::collections::HashMap;
use std::time::Instant;

struct Bucket {
    tokens: f64,
    last: Instant,
}

/// Authenticated request limiter (`limits.rest_rate_per_s` / `rest_burst`).
pub(crate) struct RateLimiter {
    rate_per_s: f64,
    burst: f64,
    buckets: HashMap<String, Bucket>,
}

impl RateLimiter {
    pub(crate) fn new(rate_per_s: f64, burst: f64) -> Self {
        Self {
            rate_per_s,
            burst: burst.max(1.0),
            buckets: HashMap::new(),
        }
    }

    /// Consume one request. `Err(retry_after_secs)` when the bucket is empty.
    pub(crate) fn check(&mut self, key: &str, now: Instant) -> Result<(), u64> {
        if self.rate_per_s <= 0.0 {
            return Ok(());
        }
        let burst = self.burst;
        let rate = self.rate_per_s;
        let bucket = self.buckets.entry(key.to_string()).or_insert(Bucket {
            tokens: burst,
            last: now,
        });
        let elapsed = now
            .checked_duration_since(bucket.last)
            .unwrap_or_default()
            .as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * rate).min(burst);
        bucket.last = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            let need = 1.0 - bucket.tokens;
            let wait = wait_secs(need / rate);
            Err(wait)
        }
    }
}

fn wait_secs(secs: f64) -> u64 {
    let ceil = secs.ceil();
    if ceil <= 1.0 {
        1
    } else {
        ceil as u64
    }
}
