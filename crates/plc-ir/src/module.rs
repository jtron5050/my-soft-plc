//! In-memory IR module and `spbc` header fields.

/// IR major version for v0.1 contract.
pub const IR_MAJOR: u16 = 0;
/// IR minor version for v0.1 contract.
pub const IR_MINOR: u16 = 1;
/// `spbc` magic bytes.
pub const SPBC_MAGIC: &[u8; 4] = b"SPBC";

/// Named entry point (task or user FB).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryPoint {
    /// Entry name (e.g. `task.main`, `fb.RS`).
    pub name: String,
    /// Byte offset into `code` (must be 4-byte aligned).
    pub pc: u32,
    /// When true, verifier requires all paths end in `RET` (user FB body).
    pub is_user_fb: bool,
}

/// Parsed / assembled IR module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrModule {
    /// IR major.
    pub ir_major: u16,
    /// IR minor.
    pub ir_minor: u16,
    /// Const segment size (bytes).
    pub const_size: u32,
    /// Data segment size (bytes).
    pub data_size: u32,
    /// Retain segment size (bytes).
    pub retain_size: u32,
    /// Number of typed `%I` slots.
    pub input_slots: u32,
    /// Number of typed `%Q` slots.
    pub output_slots: u32,
    /// Entry points.
    pub entries: Vec<EntryPoint>,
    /// Const bytes.
    pub const_data: Vec<u8>,
    /// Instruction stream bytes (little-endian u32 words).
    pub code: Vec<u8>,
}

impl IrModule {
    /// Header view used by verifier resource checks.
    #[must_use]
    pub fn header(&self) -> SpbcHeader {
        SpbcHeader {
            ir_major: self.ir_major,
            ir_minor: self.ir_minor,
            code_size: self.code.len() as u32,
            const_size: self.const_size,
            data_size: self.data_size,
            retain_size: self.retain_size,
            input_slots: self.input_slots,
            output_slots: self.output_slots,
            entry_count: self.entries.len() as u32,
        }
    }

    /// Number of instruction words.
    #[must_use]
    pub fn code_words(&self) -> usize {
        self.code.len() / 4
    }
}

/// `spbc` header fields (Appendix A.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpbcHeader {
    /// Major.
    pub ir_major: u16,
    /// Minor.
    pub ir_minor: u16,
    /// Code size bytes.
    pub code_size: u32,
    /// Const size bytes.
    pub const_size: u32,
    /// Data size bytes.
    pub data_size: u32,
    /// Retain size bytes.
    pub retain_size: u32,
    /// Input slots.
    pub input_slots: u32,
    /// Output slots.
    pub output_slots: u32,
    /// Entry count.
    pub entry_count: u32,
}
