//! Process image, quality plane, I/O mapper helpers, drivers, and double-buffers.
//!
//! Architecture PR-03: `%I`/`%Q`/`%M` + quality, scale/offset/clamp binding schema,
//! [`IoDriver`] trait, sequence-numbered double-buffer, force priority.

#![forbid(unsafe_code)]

mod double_buffer;
mod driver;
mod error;
mod force;
mod image;
mod map;
mod scale;
mod value;

pub use double_buffer::{DoubleBuffer, Snapshot};
pub use driver::{DriverDiag, InputUpdate, IoDriver, OutputImage};
pub use error::IoError;
pub use force::{
    resolve_effective_output, EffectiveOutputInput, EffectiveSource, ForceOverlay, ForceTable,
};
pub use image::{ProcessImage, SlotMeta, TypedSlot};
pub use map::{
    BadQualityPolicy, BindingDirection, ImagePlane, IoBinding, IoMap, IoModule, RawType,
    RegisterType, ValueType,
};
pub use scale::{apply_scale_offset_clamp, eng_to_raw};
pub use value::PlcValue;
