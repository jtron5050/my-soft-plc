//! Lock-free retain snapshot handoff (RT writer → T5 reader).
//!
//! Two pre-allocated byte images + an atomic sequence. The writer always
//! fills the unpublished slot, then publishes. The reader copies the
//! published slot and retries if the sequence changes mid-copy.
//!
//! v1 flushes the **whole** retain image. Page dirty-bits are out of scope.
//! Do not hold this type on the RT path during NV I/O — only `publish`
//! (bounded memcpy) belongs next to the scan thread.

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};

/// Sequence-numbered double buffer of retain bytes.
#[derive(Debug)]
pub struct RetainSnapshotBuffer {
    slots: [Box<[AtomicU8]>; 2],
    size: usize,
    /// Incremented on each successful publish (0 = never published).
    seq: AtomicU64,
    /// Published slot index (0 or 1).
    published: AtomicU64,
    /// Last sequence consumed by [`Self::read`].
    last_take: AtomicU64,
}

impl RetainSnapshotBuffer {
    /// Pre-allocate two `size`-byte slots.
    #[must_use]
    pub fn new(size: usize) -> Self {
        Self {
            slots: [zero_slot(size), zero_slot(size)],
            size,
            seq: AtomicU64::new(0),
            published: AtomicU64::new(0),
            last_take: AtomicU64::new(0),
        }
    }

    /// Image size in bytes.
    #[must_use]
    pub fn size(&self) -> usize {
        self.size
    }

    /// Current published sequence (`0` if nothing has been published).
    #[must_use]
    pub fn seq(&self) -> u64 {
        self.seq.load(Ordering::Acquire)
    }

    /// Copy `image` into the back slot and publish. Ignores a length mismatch
    /// by copying `min(len, size)` bytes (caller should pass exact size).
    pub fn publish(&self, image: &[u8]) {
        let published = self.published.load(Ordering::Relaxed) as usize;
        let back = 1 - (published & 1);
        let n = image.len().min(self.size);
        for (i, &b) in image.iter().take(n).enumerate() {
            self.slots[back][i].store(b, Ordering::Relaxed);
        }
        for i in n..self.size {
            self.slots[back][i].store(0, Ordering::Relaxed);
        }
        let next = self.seq.load(Ordering::Relaxed).wrapping_add(1).max(1);
        self.published.store(back as u64, Ordering::Release);
        self.seq.store(next, Ordering::Release);
    }

    /// Copy the published image into `dst` if it is newer than the last take.
    ///
    /// Returns the published sequence, or `None` when there is nothing new.
    /// `dst` is filled up to `min(dst.len(), size)`.
    pub fn read(&self, dst: &mut [u8]) -> Option<u64> {
        let last = self.last_take.load(Ordering::Acquire);
        let n = dst.len().min(self.size);
        let mut attempts = 0;
        loop {
            let seq = self.seq.load(Ordering::Acquire);
            if seq == 0 || seq == last {
                return None;
            }
            let slot = (self.published.load(Ordering::Acquire) as usize) & 1;
            for (i, d) in dst.iter_mut().take(n).enumerate() {
                *d = self.slots[slot][i].load(Ordering::Relaxed);
            }
            let seq2 = self.seq.load(Ordering::Acquire);
            if seq == seq2 {
                self.last_take.store(seq, Ordering::Release);
                return Some(seq);
            }
            attempts += 1;
            if attempts >= 8 {
                self.last_take.store(seq2, Ordering::Release);
                return Some(seq2);
            }
        }
    }
}

fn zero_slot(size: usize) -> Box<[AtomicU8]> {
    (0..size)
        .map(|_| AtomicU8::new(0))
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_publish_read() {
        let buf = RetainSnapshotBuffer::new(4);
        let mut dst = [0u8; 4];
        assert!(buf.read(&mut dst).is_none());

        buf.publish(&[1, 2, 3, 4]);
        let s1 = buf.read(&mut dst).expect("first");
        assert!(s1 >= 1);
        assert_eq!(dst, [1, 2, 3, 4]);
        assert!(buf.read(&mut dst).is_none());

        buf.publish(&[9, 9, 9, 9]);
        buf.publish(&[5, 6, 7, 8]);
        let s2 = buf.read(&mut dst).expect("coalesced");
        assert!(s2 > s1);
        assert_eq!(dst, [5, 6, 7, 8]);
        assert!(buf.read(&mut dst).is_none());
    }
}
