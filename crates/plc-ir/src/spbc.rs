//! `spbc` binary section framing (Appendix A.7).

use crate::error::IrError;
use crate::module::{EntryPoint, IrModule, SpbcHeader, IR_MAJOR, IR_MINOR, SPBC_MAGIC};

/// Parse a complete `spbc` blob into an [`IrModule`].
pub fn parse_spbc(bytes: &[u8]) -> Result<IrModule, IrError> {
    if bytes.len() < 36 {
        return Err(IrError::Spbc("buffer too short for header".into()));
    }
    if &bytes[0..4] != SPBC_MAGIC {
        return Err(IrError::Spbc("bad magic".into()));
    }
    let ir_major = u16::from_le_bytes(bytes[4..6].try_into().unwrap());
    let ir_minor = u16::from_le_bytes(bytes[6..8].try_into().unwrap());
    let code_size = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    let const_size = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
    let data_size = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
    let retain_size = u32::from_le_bytes(bytes[20..24].try_into().unwrap());
    let input_slots = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
    let output_slots = u32::from_le_bytes(bytes[28..32].try_into().unwrap());
    let entry_count = u32::from_le_bytes(bytes[32..36].try_into().unwrap());

    let mut off = 36usize;
    let mut entries = Vec::with_capacity(entry_count as usize);
    for _ in 0..entry_count {
        if off >= bytes.len() {
            return Err(IrError::Spbc("truncated entry table".into()));
        }
        let name_len = bytes[off] as usize;
        off += 1;
        if off + name_len + 4 > bytes.len() {
            return Err(IrError::Spbc("truncated entry name/pc".into()));
        }
        let name = std::str::from_utf8(&bytes[off..off + name_len])
            .map_err(|_| IrError::Spbc("entry name not utf-8".into()))?
            .to_string();
        off += name_len;
        let pc = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
        off += 4;
        let is_user_fb = name.starts_with("fb.") || name.starts_with("FB.");
        entries.push(EntryPoint {
            name,
            pc,
            is_user_fb,
        });
    }

    if off + const_size as usize + code_size as usize > bytes.len() {
        return Err(IrError::Spbc("truncated const/code".into()));
    }
    let const_data = bytes[off..off + const_size as usize].to_vec();
    off += const_size as usize;
    let code = bytes[off..off + code_size as usize].to_vec();

    if ir_major != IR_MAJOR || ir_minor != IR_MINOR {
        // Still parse; verifier will reject unknown major for execution.
    }

    let _header = SpbcHeader {
        ir_major,
        ir_minor,
        code_size,
        const_size,
        data_size,
        retain_size,
        input_slots,
        output_slots,
        entry_count,
    };

    Ok(IrModule {
        ir_major,
        ir_minor,
        const_size,
        data_size,
        retain_size,
        input_slots,
        output_slots,
        entries,
        const_data,
        code,
    })
}

/// Serialize an [`IrModule`] to `spbc` bytes.
pub fn write_spbc(module: &IrModule) -> Result<Vec<u8>, IrError> {
    if module.code.len() as u32 != module.header().code_size && module.code.len() % 4 != 0 {
        // code_size derived from code len
    }
    let mut out = Vec::new();
    out.extend_from_slice(SPBC_MAGIC);
    out.extend_from_slice(&module.ir_major.to_le_bytes());
    out.extend_from_slice(&module.ir_minor.to_le_bytes());
    let code_size = module.code.len() as u32;
    let const_size = module.const_data.len() as u32;
    out.extend_from_slice(&code_size.to_le_bytes());
    out.extend_from_slice(&const_size.to_le_bytes());
    out.extend_from_slice(&module.data_size.to_le_bytes());
    out.extend_from_slice(&module.retain_size.to_le_bytes());
    out.extend_from_slice(&module.input_slots.to_le_bytes());
    out.extend_from_slice(&module.output_slots.to_le_bytes());
    out.extend_from_slice(&(module.entries.len() as u32).to_le_bytes());

    for e in &module.entries {
        if e.name.len() > 255 {
            return Err(IrError::Spbc("entry name too long".into()));
        }
        out.push(e.name.len() as u8);
        out.extend_from_slice(e.name.as_bytes());
        out.extend_from_slice(&e.pc.to_le_bytes());
    }
    out.extend_from_slice(&module.const_data);
    out.extend_from_slice(&module.code);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asm::assemble;

    #[test]
    fn round_trip_minimal() {
        let src = r#"
.header data_size=32 retain_size=0 input_slots=0 output_slots=0
.entry task.main
HALT
"#;
        let m = assemble(src).unwrap();
        let bytes = write_spbc(&m).unwrap();
        let m2 = parse_spbc(&bytes).unwrap();
        assert_eq!(m2.entries[0].name, "task.main");
        assert_eq!(m2.code, m.code);
    }
}
