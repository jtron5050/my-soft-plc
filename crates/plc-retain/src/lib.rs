//! Symbolic retain memory store (architecture PR-08).
//!
//! Non-RT crate: encode + `fsync` belong on the T5 flusher thread. The RT
//! scan path only sets a dirty flag (`plc-scan::RetainDirtyWatch`) and,
//! later, publishes bytes into [`RetainSnapshotBuffer`].
//!
//! # On-disk layout
//!
//! Architecture names the store `/var/lib/soft-plc/retain/<program_id>.ret`.
//! That is the **logical** name. Physically the store is A/B + index:
//!
//! - `<program_id>.ret.0` / `<program_id>.ret.1` — checksummed symbolic slots
//! - `<program_id>.ret.idx` — committed slot and generation
//!
//! Payload records are `(name, type, value)`. Offsets live only in the
//! in-memory [`plc_ir::RetainLayout`] used to pack the VM retain segment.
//!
//! # Corruption
//!
//! A bad CRC, magic, or version discards that slot. If both slots are
//! unusable the load path **cold-starts** (zeros) and reports via
//! [`LoadReport`] — it does not FAULT (KD-17). The last-fsync dirty window
//! is accepted (KD-23).
//!
//! # PR-10 (not implemented here)
//!
//! On arm, non-RT: [`map_retain`] builds a shadow image. On activate CS:
//! pointer-swing + bounded `memcpy`. On boot: [`RetainStore::load`]. On
//! dirty: scan publishes a snapshot; T5 [`RetainStore::flush`]. Graceful
//! shutdown: one extra flush.

#![forbid(unsafe_code)]

mod codec;
mod crc;
mod error;
mod layout;
mod map;
mod snapshot;
mod store;

pub use codec::{decode_records, encode_records, records_from_image, RetainRecord};
pub use error::RetainError;
pub use layout::schema_hash;
pub use map::{apply_records, map_retain, MapReport, MappedRetain};
pub use snapshot::RetainSnapshotBuffer;
pub use store::{
    validate_program_id, FlushReport, LoadReport, LoadSource, RetainStore, IDX_LEN, IDX_MAGIC,
    RETAIN_VERSION, SLOT_HEADER_LEN, SLOT_MAGIC,
};
