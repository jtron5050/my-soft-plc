//! In-memory diagnostics event ring (architecture 4096; file export is PR-18).

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;

/// Diagnostics ring capacity.
pub const EVENT_CAP: usize = 4096;

/// One diagnostics event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiagEvent {
    /// Monotonic sequence (oldest kept on overflow).
    pub seq: u64,
    /// Wall-clock unix seconds.
    pub unix_secs: u64,
    /// Event kind.
    pub kind: String,
    /// Free-form detail.
    pub detail: String,
}

/// Overwrite-oldest ring.
#[derive(Debug)]
pub struct EventRing {
    cap: usize,
    next_seq: Mutex<u64>,
    events: Mutex<VecDeque<DiagEvent>>,
}

impl EventRing {
    /// Capacity [`EVENT_CAP`].
    #[must_use]
    pub fn new() -> Self {
        Self::with_cap(EVENT_CAP)
    }

    /// Custom capacity (tests).
    #[must_use]
    pub fn with_cap(cap: usize) -> Self {
        Self {
            cap: cap.max(1),
            next_seq: Mutex::new(1),
            events: Mutex::new(VecDeque::new()),
        }
    }

    /// Append.
    pub fn push(&self, unix_secs: u64, kind: impl Into<String>, detail: impl Into<String>) {
        let seq = {
            let mut n = self.next_seq.lock().expect("event seq");
            let s = *n;
            *n += 1;
            s
        };
        let ev = DiagEvent {
            seq,
            unix_secs,
            kind: kind.into(),
            detail: detail.into(),
        };
        let mut q = self.events.lock().expect("event ring");
        if q.len() >= self.cap {
            q.pop_front();
        }
        q.push_back(ev);
    }

    /// Snapshot oldest-first.
    #[must_use]
    pub fn snapshot(&self) -> Vec<DiagEvent> {
        self.events
            .lock()
            .expect("event ring")
            .iter()
            .cloned()
            .collect()
    }
}

impl Default for EventRing {
    fn default() -> Self {
        Self::new()
    }
}
