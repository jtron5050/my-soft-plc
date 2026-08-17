//! RT → non-RT sample ring.
//!
//! The crate forbids `unsafe`, so this is not a classic atomic SPSC. The
//! producer uses **`try_lock` only** and never waits: if the consumer holds
//! the mutex (a Copy pop, no I/O), the sample is dropped. A full ring drops
//! the oldest sample. That matches the architecture rule: telemetry
//! backpressure must not block the scan.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Shared ring.
struct Inner<T> {
    q: Mutex<VecDeque<T>>,
    cap: usize,
    drops: AtomicU64,
}

/// Producer half (scan thread).
#[derive(Clone)]
pub struct SpscProducer<T> {
    inner: Arc<Inner<T>>,
}

/// Consumer half (non-RT).
#[derive(Clone)]
pub struct SpscConsumer<T> {
    inner: Arc<Inner<T>>,
}

/// Allocate a ring with at least `capacity` slots (minimum 1).
#[must_use]
pub fn channel<T>(capacity: usize) -> (SpscProducer<T>, SpscConsumer<T>) {
    let cap = capacity.max(1);
    let mut q = VecDeque::new();
    q.reserve_exact(cap);
    let inner = Arc::new(Inner {
        q: Mutex::new(q),
        cap,
        drops: AtomicU64::new(0),
    });
    (
        SpscProducer {
            inner: Arc::clone(&inner),
        },
        SpscConsumer { inner },
    )
}

impl<T> SpscProducer<T> {
    /// Push `value`. Never blocks.
    ///
    /// * Full ring → drop oldest, count a drop, then push.
    /// * `try_lock` fail → drop this sample (count a drop).
    pub fn push_drop_oldest(&self, value: T) {
        let inner = &self.inner;
        let Ok(mut q) = inner.q.try_lock() else {
            inner.drops.fetch_add(1, Ordering::Relaxed);
            return;
        };
        if q.len() >= inner.cap {
            q.pop_front();
            inner.drops.fetch_add(1, Ordering::Relaxed);
        }
        q.push_back(value);
    }
}

impl<T> SpscConsumer<T> {
    /// Pop the oldest sample, if any.
    #[must_use]
    pub fn try_recv(&self) -> Option<T> {
        let mut q = self
            .inner
            .q
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        q.pop_front()
    }

    /// Cumulative producer-side drops.
    #[must_use]
    pub fn drops(&self) -> u64 {
        self.inner.drops.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_pop() {
        let (tx, rx) = channel::<u32>(2);
        tx.push_drop_oldest(1);
        tx.push_drop_oldest(2);
        assert_eq!(rx.try_recv(), Some(1));
        assert_eq!(rx.try_recv(), Some(2));
        assert_eq!(rx.try_recv(), None);
    }

    #[test]
    fn drop_oldest_when_full() {
        let (tx, rx) = channel::<u32>(2);
        tx.push_drop_oldest(1);
        tx.push_drop_oldest(2);
        tx.push_drop_oldest(3);
        assert_eq!(rx.drops(), 1);
        assert_eq!(rx.try_recv(), Some(2));
        assert_eq!(rx.try_recv(), Some(3));
    }
}
