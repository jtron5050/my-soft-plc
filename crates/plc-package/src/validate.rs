//! Upload-time validation: signature, one `spbc` section, IR verify, cross-checks.

use plc_ir::{verify_module, IR_MAJOR, IR_MINOR};

use crate::compat::compute_compatibility_hash;
use crate::error::PackageError;
use crate::format::{parse_spkg, ParsedPackage};
use crate::sign::{check_policy, signing_preimage, VerifyPolicy};

/// Parse, verify signature policy, require one section, verify IR, cross-check.
pub fn validate(bytes: &[u8], policy: VerifyPolicy<'_>) -> Result<ParsedPackage, PackageError> {
    let parsed = parse_spkg(bytes)?;
    let preimage = signing_preimage(&parsed.manifest_canonical, &parsed.sections);
    check_policy(policy, &parsed.signature, &preimage)?;

    let count = u32::try_from(parsed.sections.len()).unwrap_or(u32::MAX);
    if parsed.sections.len() != 1 {
        return Err(PackageError::SectionCount(count));
    }

    let module = &parsed.modules[0];
    verify_module(module)?;
    cross_check(&parsed)?;

    let expected = compute_compatibility_hash(&parsed.manifest);
    if expected != parsed.manifest.compatibility_hash {
        return Err(PackageError::CompatibilityHash);
    }
    Ok(parsed)
}

fn cross_check(parsed: &ParsedPackage) -> Result<(), PackageError> {
    let module = &parsed.modules[0];
    let m = &parsed.manifest;

    if m.ir_major != IR_MAJOR || m.ir_minor != IR_MINOR {
        return Err(PackageError::mismatch(format!(
            "manifest ir {}/{} != runtime {}/{}",
            m.ir_major, m.ir_minor, IR_MAJOR, IR_MINOR
        )));
    }
    if m.ir_major != module.ir_major || m.ir_minor != module.ir_minor {
        return Err(PackageError::mismatch(format!(
            "manifest ir {}/{} != spbc {}/{}",
            m.ir_major, m.ir_minor, module.ir_major, module.ir_minor
        )));
    }
    if let Some(n) = m.input_slots {
        if n != module.input_slots {
            return Err(PackageError::mismatch(format!(
                "input_slots {n} != spbc {}",
                module.input_slots
            )));
        }
    }
    if let Some(n) = m.output_slots {
        if n != module.output_slots {
            return Err(PackageError::mismatch(format!(
                "output_slots {n} != spbc {}",
                module.output_slots
            )));
        }
    }
    if let Some(n) = m.data_size {
        if n != module.data_size {
            return Err(PackageError::mismatch(format!(
                "data_size {n} != spbc {}",
                module.data_size
            )));
        }
    }
    if let Some(n) = m.retain_size {
        if n != module.retain_size {
            return Err(PackageError::mismatch(format!(
                "retain_size {n} != spbc {}",
                module.retain_size
            )));
        }
    }
    if let Some(n) = m.const_size {
        if n != module.const_size {
            return Err(PackageError::mismatch(format!(
                "const_size {n} != spbc {}",
                module.const_size
            )));
        }
    }

    for (task, symbol) in &m.task_entries {
        if !module.entries.iter().any(|e| e.name == *symbol) {
            return Err(PackageError::mismatch(format!(
                "task_entries.{task} symbol {symbol:?} not in spbc"
            )));
        }
    }

    // Offsets / overlap / OOB against the authoritative retain_size.
    m.retain_layout(module.retain_size)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::MAX_PACKAGE_BYTES;

    #[test]
    fn too_large_short_circuits() {
        let n = MAX_PACKAGE_BYTES + 1;
        let err = validate(&vec![0u8; n], VerifyPolicy::unsigned()).unwrap_err();
        assert_eq!(err, PackageError::TooLarge(n));
    }
}
