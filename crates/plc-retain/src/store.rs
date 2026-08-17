//! Double-buffered NV store (`<id>.ret.0` / `.ret.1` / `.ret.idx`).

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use plc_ir::RetainLayout;

use crate::codec::{decode_records, encode_records};
use crate::crc::crc32;
use crate::error::RetainError;
use crate::layout::schema_hash;
use crate::map::apply_records;

/// Slot file magic (`SPRT` — Soft PLC ReTain).
pub const SLOT_MAGIC: &[u8; 4] = b"SPRT";
/// Index file magic (`SPRI`).
pub const IDX_MAGIC: &[u8; 4] = b"SPRI";
/// On-disk format version.
pub const RETAIN_VERSION: u16 = 1;
/// Slot header size in bytes.
pub const SLOT_HEADER_LEN: usize = 32;
/// Index file size in bytes.
pub const IDX_LEN: usize = 64;

/// Which A/B slot supplied the image (or cold start).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadSource {
    /// No usable NV; destination zeroed.
    Cold,
    /// Slot 0.
    Slot0,
    /// Slot 1.
    Slot1,
}

impl LoadSource {
    fn from_slot(slot: u8) -> Self {
        if slot == 0 {
            Self::Slot0
        } else {
            Self::Slot1
        }
    }
}

/// Result of [`RetainStore::load`].
///
/// Corruption and a missing store are **not** errors: the controller boots
/// with zeros (KD-17 / KD-23). Callers log [`Self::corrupt`] / [`Self::missing`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadReport {
    /// Where the bytes came from.
    pub source: LoadSource,
    /// Generation of the chosen slot (`0` on cold).
    pub generation: u64,
    /// Symbols kept from NV.
    pub kept: u32,
    /// New symbols left at zero.
    pub cold_defaults: u32,
    /// NV symbols not in the requested layout.
    pub dropped: u32,
    /// NV symbols whose type disagreed (zeroed; load never rejects).
    pub incompat: Vec<String>,
    /// At least one slot or the idx looked present but failed checks.
    pub corrupt: bool,
    /// No slot files existed.
    pub missing: bool,
}

/// Result of [`RetainStore::flush`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlushReport {
    /// New committed generation.
    pub generation: u64,
    /// Slot that was written (`0` or `1`).
    pub slot: u8,
    /// Bytes written to the slot file (header + payload).
    pub bytes_written: usize,
}

/// Symbolic retain store under a directory.
///
/// Logical name in the architecture is `<program_id>.ret`. On disk that is
/// three siblings:
/// - `<program_id>.ret.0` / `<program_id>.ret.1` — A/B payload slots
/// - `<program_id>.ret.idx` — committed slot + generation
///
/// T5 (non-RT) calls [`Self::flush`] after copying a published snapshot.
/// The RT scan thread must never call this type.
///
/// # PR-10 contract
///
/// - Boot: `load(program_id, layout, vm_retain_bytes)`.
/// - Arm (non-RT): [`crate::map_retain`] builds the shadow image.
/// - Activate CS: pointer-swing + bounded `memcpy` of that image.
/// - Dirty: scan publishes a snapshot; T5 `flush`. Graceful shutdown: one extra flush.
#[derive(Debug, Clone)]
pub struct RetainStore {
    dir: PathBuf,
}

impl RetainStore {
    /// Create `dir` if needed and return a store rooted there.
    pub fn open(dir: impl AsRef<Path>) -> Result<Self, RetainError> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir).map_err(|e| RetainError::io(&dir, e))?;
        Ok(Self { dir })
    }

    /// Store directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Logical `<dir>/<program_id>.ret` (documentation name; not a real file).
    pub fn path_for(&self, program_id: &str) -> Result<PathBuf, RetainError> {
        validate_program_id(program_id)?;
        Ok(self.dir.join(format!("{program_id}.ret")))
    }

    /// True when any slot file exists for `program_id`.
    #[must_use]
    pub fn exists(&self, program_id: &str) -> bool {
        if validate_program_id(program_id).is_err() {
            return false;
        }
        self.slot_path(program_id, 0).is_ok_and(|p| p.exists())
            || self.slot_path(program_id, 1).is_ok_and(|p| p.exists())
    }

    /// Encode + write the inactive slot + fsync + commit idx + fsync.
    pub fn flush(
        &self,
        program_id: &str,
        layout: &RetainLayout,
        image: &[u8],
    ) -> Result<FlushReport, RetainError> {
        validate_program_id(program_id)?;
        if image.len() != layout.retain_size as usize {
            return Err(RetainError::ImageSize {
                expected: layout.retain_size,
                actual: image.len(),
            });
        }
        let payload = encode_records(layout, image)?;
        let (active, gen) = self.committed(program_id);
        let slot = 1 - active;
        let generation = gen.saturating_add(1).max(1);
        let schema = schema_hash(layout);
        let bytes = encode_slot(generation, schema, layout.symbols.len() as u32, &payload);
        let path = self.slot_path(program_id, slot)?;
        write_all_sync(&path, &bytes)?;
        let idx_path = self.idx_path(program_id)?;
        write_all_sync(&idx_path, &encode_idx(slot, generation))?;
        sync_dir(&self.dir);
        Ok(FlushReport {
            generation,
            slot,
            bytes_written: bytes.len(),
        })
    }

    /// Load NV into `dst` using `layout`. Missing or corrupt → zeros + report.
    ///
    /// Hard-errors only on an invalid `program_id` or `dst` length.
    pub fn load(
        &self,
        program_id: &str,
        layout: &RetainLayout,
        dst: &mut [u8],
    ) -> Result<LoadReport, RetainError> {
        validate_program_id(program_id)?;
        if dst.len() != layout.retain_size as usize {
            return Err(RetainError::ImageSize {
                expected: layout.retain_size,
                actual: dst.len(),
            });
        }
        dst.fill(0);

        let slot0 = self.read_slot(program_id, 0);
        let slot1 = self.read_slot(program_id, 1);
        let any_file = self.exists(program_id) || self.idx_path(program_id)?.exists();
        let idx = self.read_idx(program_id);
        let idx_ok = idx.is_some();
        let idx_bad = self.idx_path(program_id)?.exists() && !idx_ok;

        let chosen = choose_slot(idx.as_ref(), slot0.as_ref().ok(), slot1.as_ref().ok());
        let saw_corrupt = idx_bad
            || matches!(slot0, Err(SlotRead::Corrupt))
            || matches!(slot1, Err(SlotRead::Corrupt))
            || (idx_ok && chosen.is_none());

        let Some((slot, decoded)) = chosen else {
            return Ok(LoadReport {
                source: LoadSource::Cold,
                generation: 0,
                kept: 0,
                cold_defaults: layout.symbols.len() as u32,
                dropped: 0,
                incompat: Vec::new(),
                corrupt: saw_corrupt,
                missing: !any_file,
            });
        };

        // Load never rejects type mismatch: boot must proceed (zeros + report).
        let mapped = apply_records(&decoded.records, layout, true)?;
        dst.copy_from_slice(&mapped.image);
        Ok(LoadReport {
            source: LoadSource::from_slot(slot),
            generation: decoded.generation,
            kept: mapped.report.kept,
            cold_defaults: mapped.report.cold_defaults,
            dropped: mapped.report.dropped,
            incompat: mapped.report.zeroed_incompat,
            corrupt: saw_corrupt,
            missing: false,
        })
    }

    fn slot_path(&self, program_id: &str, slot: u8) -> Result<PathBuf, RetainError> {
        validate_program_id(program_id)?;
        Ok(self.dir.join(format!("{program_id}.ret.{slot}")))
    }

    fn idx_path(&self, program_id: &str) -> Result<PathBuf, RetainError> {
        validate_program_id(program_id)?;
        Ok(self.dir.join(format!("{program_id}.ret.idx")))
    }

    fn committed(&self, program_id: &str) -> (u8, u64) {
        if let Some(idx) = self.read_idx(program_id) {
            return (idx.slot, idx.generation);
        }
        let s0 = self.read_slot(program_id, 0).ok();
        let s1 = self.read_slot(program_id, 1).ok();
        match (s0, s1) {
            (Some(a), Some(b)) => {
                if a.generation >= b.generation {
                    (0, a.generation)
                } else {
                    (1, b.generation)
                }
            }
            (Some(a), None) => (0, a.generation),
            (None, Some(b)) => (1, b.generation),
            (None, None) => (1, 0), // next flush writes slot 0, gen 1
        }
    }

    fn read_idx(&self, program_id: &str) -> Option<IdxRecord> {
        let path = self.idx_path(program_id).ok()?;
        let bytes = fs::read(&path).ok()?;
        decode_idx(&bytes)
    }

    fn read_slot(&self, program_id: &str, slot: u8) -> Result<DecodedSlot, SlotRead> {
        let path = self
            .slot_path(program_id, slot)
            .map_err(|_| SlotRead::Missing)?;
        if !path.exists() {
            return Err(SlotRead::Missing);
        }
        let bytes = fs::read(&path).map_err(|_| SlotRead::Corrupt)?;
        decode_slot(&bytes).ok_or(SlotRead::Corrupt)
    }
}

#[derive(Debug)]
enum SlotRead {
    Missing,
    Corrupt,
}

struct IdxRecord {
    slot: u8,
    generation: u64,
}

struct DecodedSlot {
    generation: u64,
    records: Vec<crate::codec::RetainRecord>,
}

fn choose_slot(
    idx: Option<&IdxRecord>,
    slot0: Option<&DecodedSlot>,
    slot1: Option<&DecodedSlot>,
) -> Option<(u8, DecodedSlot)> {
    if let Some(idx) = idx {
        let preferred = if idx.slot == 0 { slot0 } else { slot1 };
        if let Some(s) = preferred {
            if s.generation == idx.generation {
                return Some(clone_choice(idx.slot, s));
            }
        }
        // Idx points at a bad slot — fall back to the other if valid.
        let other = if idx.slot == 0 { slot1 } else { slot0 };
        let other_id = 1 - idx.slot;
        if let Some(s) = other {
            return Some(clone_choice(other_id, s));
        }
        return None;
    }
    match (slot0, slot1) {
        (Some(a), Some(b)) => {
            if a.generation >= b.generation {
                Some(clone_choice(0, a))
            } else {
                Some(clone_choice(1, b))
            }
        }
        (Some(a), None) => Some(clone_choice(0, a)),
        (None, Some(b)) => Some(clone_choice(1, b)),
        (None, None) => None,
    }
}

fn clone_choice(slot: u8, s: &DecodedSlot) -> (u8, DecodedSlot) {
    (
        slot,
        DecodedSlot {
            generation: s.generation,
            records: s.records.clone(),
        },
    )
}

/// Accept a single `[A-Za-z0-9._-]+` path segment.
pub fn validate_program_id(id: &str) -> Result<(), RetainError> {
    if id.is_empty()
        || id == "."
        || id == ".."
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
    {
        return Err(RetainError::InvalidProgramId(id.to_string()));
    }
    Ok(())
}

fn encode_slot(generation: u64, schema_crc: u32, record_count: u32, payload: &[u8]) -> Vec<u8> {
    let mut buf = vec![0u8; SLOT_HEADER_LEN + payload.len()];
    buf[0..4].copy_from_slice(SLOT_MAGIC);
    buf[4..6].copy_from_slice(&RETAIN_VERSION.to_le_bytes());
    buf[6..8].copy_from_slice(&0u16.to_le_bytes());
    buf[8..16].copy_from_slice(&generation.to_le_bytes());
    buf[16..20].copy_from_slice(&schema_crc.to_le_bytes());
    buf[20..24].copy_from_slice(&record_count.to_le_bytes());
    buf[24..28].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    // crc field left zero while hashing
    buf[32..].copy_from_slice(payload);
    let crc = crc32(&buf);
    buf[28..32].copy_from_slice(&crc.to_le_bytes());
    buf
}

fn decode_slot(bytes: &[u8]) -> Option<DecodedSlot> {
    if bytes.len() < SLOT_HEADER_LEN {
        return None;
    }
    if &bytes[0..4] != SLOT_MAGIC {
        return None;
    }
    let version = u16::from_le_bytes(bytes[4..6].try_into().ok()?);
    if version != RETAIN_VERSION {
        return None;
    }
    let generation = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
    let payload_len = u32::from_le_bytes(bytes[24..28].try_into().ok()?) as usize;
    if bytes.len() != SLOT_HEADER_LEN + payload_len {
        return None;
    }
    let stored_crc = u32::from_le_bytes(bytes[28..32].try_into().ok()?);
    let mut hashed = bytes.to_vec();
    hashed[28..32].copy_from_slice(&[0, 0, 0, 0]);
    if crc32(&hashed) != stored_crc {
        return None;
    }
    let records = decode_records(&bytes[SLOT_HEADER_LEN..]).ok()?;
    Some(DecodedSlot {
        generation,
        records,
    })
}

fn encode_idx(slot: u8, generation: u64) -> Vec<u8> {
    let mut buf = vec![0u8; IDX_LEN];
    buf[0..4].copy_from_slice(IDX_MAGIC);
    buf[4..6].copy_from_slice(&RETAIN_VERSION.to_le_bytes());
    buf[6] = slot;
    buf[8..16].copy_from_slice(&generation.to_le_bytes());
    let crc = crc32(&buf[0..16]);
    buf[16..20].copy_from_slice(&crc.to_le_bytes());
    buf
}

fn decode_idx(bytes: &[u8]) -> Option<IdxRecord> {
    if bytes.len() != IDX_LEN {
        return None;
    }
    if &bytes[0..4] != IDX_MAGIC {
        return None;
    }
    let version = u16::from_le_bytes(bytes[4..6].try_into().ok()?);
    if version != RETAIN_VERSION {
        return None;
    }
    let slot = bytes[6];
    if slot > 1 {
        return None;
    }
    let generation = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
    let stored = u32::from_le_bytes(bytes[16..20].try_into().ok()?);
    if crc32(&bytes[0..16]) != stored {
        return None;
    }
    Some(IdxRecord { slot, generation })
}

fn write_all_sync(path: &Path, bytes: &[u8]) -> Result<(), RetainError> {
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|e| RetainError::io(path, e))?;
    f.write_all(bytes).map_err(|e| RetainError::io(path, e))?;
    f.sync_all().map_err(|e| RetainError::io(path, e))?;
    Ok(())
}

fn sync_dir(dir: &Path) {
    if let Ok(f) = File::open(dir) {
        let _ = f.sync_all();
    }
}

impl RetainStore {
    /// Absolute path of slot `0` or `1` (tests / diagnostics).
    pub fn slot_file(&self, program_id: &str, slot: u8) -> Result<PathBuf, RetainError> {
        self.slot_path(program_id, slot)
    }

    /// Absolute path of the idx file (tests / diagnostics).
    pub fn idx_file(&self, program_id: &str) -> Result<PathBuf, RetainError> {
        self.idx_path(program_id)
    }
}
