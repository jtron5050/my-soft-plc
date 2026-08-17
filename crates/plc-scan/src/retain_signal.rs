//! Coalescing dirty-retain signal (RT → T5 flusher, PR-08).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// Event consumed by the retain flusher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetainDirtyEvent {
    /// Monotonic dirty generation (increments on each ST_RETAIN burst).
    pub seq: u64,
}

/// Shared dirty flag + sequence.
#[derive(Debug, Clone)]
pub struct RetainDirtySignal {
    dirty: Arc<AtomicBool>,
    seq: Arc<AtomicU64>,
}

impl RetainDirtySignal {
    /// Clean signal at seq 0.
    #[must_use]
    pub fn new() -> Self {
        Self {
            dirty: Arc::new(AtomicBool::new(false)),
            seq: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Mark retain dirty (scan path after Logic).
    pub fn notify(&self) {
        self.seq.fetch_add(1, Ordering::Release);
        self.dirty.store(true, Ordering::Release);
    }

    /// Consumer handle (cloneable).
    #[must_use]
    pub fn watch(&self) -> RetainDirtyWatch {
        RetainDirtyWatch {
            dirty: Arc::clone(&self.dirty),
            seq: Arc::clone(&self.seq),
        }
    }
}

impl Default for RetainDirtySignal {
    fn default() -> Self {
        Self::new()
    }
}

/// Non-RT consumer of retain-dirty notifications.
#[derive(Debug, Clone)]
pub struct RetainDirtyWatch {
    dirty: Arc<AtomicBool>,
    seq: Arc<AtomicU64>,
}

impl RetainDirtyWatch {
    /// Current sequence (even if not dirty).
    #[must_use]
    pub fn seq(&self) -> u64 {
        self.seq.load(Ordering::Acquire)
    }

    /// Swap dirty=false and return the last seq when a write happened.
    #[must_use]
    pub fn take(&self) -> Option<RetainDirtyEvent> {
        let was = self.dirty.swap(false, Ordering::AcqRel);
        if was {
            Some(RetainDirtyEvent { seq: self.seq() })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_coalesces() {
        let s = RetainDirtySignal::new();
        let w = s.watch();
        assert!(w.take().is_none());
        s.notify();
        s.notify();
        let ev = w.take().expect("dirty");
        assert_eq!(ev.seq, 2);
        assert!(w.take().is_none());
    }
}
