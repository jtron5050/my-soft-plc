//! IR v0.1: types, `spbc` framing, verifier, and `spasm` text assembler.
//!
//! Appendix A of the architecture design is normative. Golden fixtures under
//! `tests/fixtures/` are the review oracle for opcode encoding.

#![forbid(unsafe_code)]

mod asm;
mod encode;
mod error;
mod module;
mod opcode;
mod retain;
mod spbc;
mod value;
mod verify;

pub use asm::{assemble, AssembleOptions};
pub use encode::{decode_instruction, encode_instruction, pack_word, DecodedInstr};
pub use error::{IrError, VerifyError};
pub use module::{EntryPoint, IrModule, SpbcHeader, IR_MAJOR, IR_MINOR, SPBC_MAGIC};
pub use opcode::{Opcode, PrimitiveId};
pub use retain::{RetainLayout, RetainSymbol};
pub use spbc::{parse_spbc, write_spbc};
pub use value::IrType;
pub use verify::verify_module;
