//! Sequence-numbered double-buffer handoff (RT ↔ non-RT workers).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use plc_types::Quality;

use crate::value::PlcValue;

/// One published snapshot of remote inputs (or outputs for workers).
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Monotonic sequence; odd means write in progress (optional protocol).
    pub seq: u64,
    /// Slot values.
    pub values: Vec<PlcValue>,
    /// Parallel quality plane.
    pub quality: Vec<Quality>,
}

impl Snapshot {
    /// Empty snapshot with `n` Good/zero BOOL slots.
    #[must_use]
    pub fn zeros(n: usize) -> Self {
        Self {
            seq: 0,
            values: vec![PlcValue::Bool(false); n],
            quality: vec![Quality::Good; n],
        }
    }
}

/// Double-buffer with sequence protocol for torn-read avoidance.
///
/// Writer: fill back buffer, then `publish`. Reader: `read` copies front; if
/// `seq` changes mid-copy, retry (bounded).
#[derive(Debug)]
pub struct DoubleBuffer {
    front: Mutex<Snapshot>,
    back: Mutex<Snapshot>,
    seq: AtomicU64,
    slots: usize,
}

impl DoubleBuffer {
    /// Create buffers with `slots` elements each.
    #[must_use]
    pub fn new(slots: usize) -> Arc<Self> {
        Arc::new(Self {
            front: Mutex::new(Snapshot::zeros(slots)),
            back: Mutex::new(Snapshot::zeros(slots)),
            seq: AtomicU64::new(0),
            slots,
        })
    }

    /// Slot capacity.
    #[must_use]
    pub fn slots(&self) -> usize {
        self.slots
    }

    /// Writer path: replace back buffer and publish as front.
    pub fn publish(&self, values: Vec<PlcValue>, quality: Vec<Quality>) {
        assert_eq!(values.len(), self.slots);
        assert_eq!(quality.len(), self.slots);
        let next = self.seq.load(Ordering::Relaxed).wrapping_add(1);
        {
            let mut back = self
                .back
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            back.seq = next;
            back.values = values;
            back.quality = quality;
        }
        // Swap front/back under both locks (non-RT or RT-safe if locks are short).
        let mut front = self
            .front
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut back = self
            .back
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::mem::swap(&mut *front, &mut *back);
        self.seq.store(next, Ordering::Release);
    }

    /// Reader path with bounded seq-consistency retries.
    #[must_use]
    pub fn read(&self, max_retries: u32) -> Snapshot {
        let mut attempts = 0;
        loop {
            let seq_before = self.seq.load(Ordering::Acquire);
            let snap = {
                let front = self
                    .front
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                front.clone()
            };
            let seq_after = self.seq.load(Ordering::Acquire);
            if seq_before == seq_after && snap.seq == seq_after {
                return snap;
            }
            attempts += 1;
            if attempts >= max_retries {
                return snap;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_and_read() {
        let db = DoubleBuffer::new(2);
        db.publish(
            vec![PlcValue::Bool(true), PlcValue::Real(1.5)],
            vec![Quality::Good, Quality::Uncertain],
        );
        let s = db.read(8);
        assert_eq!(s.values[0], PlcValue::Bool(true));
        assert_eq!(s.quality[1], Quality::Uncertain);
        assert!(s.seq >= 1);
    }

    #[test]
    fn sequence_advances() {
        let db = DoubleBuffer::new(1);
        db.publish(vec![PlcValue::Bool(false)], vec![Quality::Good]);
        let s1 = db.read(4).seq;
        db.publish(vec![PlcValue::Bool(true)], vec![Quality::Bad]);
        let s2 = db.read(4).seq;
        assert!(s2 > s1);
    }
}
