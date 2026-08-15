//! Maintenance force overlays and effective-output resolution.

use std::collections::BTreeMap;

use plc_types::{OperatingMode, Quality};

use crate::map::BadQualityPolicy;
use crate::value::PlcValue;

/// Active force for a single output tag / slot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ForceOverlay {
    /// Forced value.
    pub value: PlcValue,
}

/// Table of forced outputs keyed by output slot index.
#[derive(Debug, Clone, Default)]
pub struct ForceTable {
    forces: BTreeMap<u32, ForceOverlay>,
}

impl ForceTable {
    /// Empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set or replace a force.
    pub fn set(&mut self, slot: u32, value: PlcValue) {
        self.forces.insert(slot, ForceOverlay { value });
    }

    /// Clear one force.
    pub fn clear(&mut self, slot: u32) {
        self.forces.remove(&slot);
    }

    /// Clear all forces (STOP / FAULT / FAULT_RESET).
    pub fn clear_all(&mut self) {
        self.forces.clear();
    }

    /// Lookup.
    #[must_use]
    pub fn get(&self, slot: u32) -> Option<ForceOverlay> {
        self.forces.get(&slot).copied()
    }

    /// Whether any force is active.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.forces.is_empty()
    }
}

/// Inputs for effective output resolution (architecture write / force priority).
#[derive(Debug, Clone, Copy)]
pub struct EffectiveOutputInput {
    /// Operating mode.
    pub mode: OperatingMode,
    /// Global force-safe (FAULT path or explicit).
    pub global_force_safe: bool,
    /// Module quality for this output's module.
    pub module_quality: Quality,
    /// Module policy on bad quality.
    pub on_bad_quality: BadQualityPolicy,
    /// Maintenance force, if any.
    pub force: Option<ForceOverlay>,
    /// Last program `%Q` value.
    pub program_value: PlcValue,
    /// Whether the program has written this slot since cold/arm.
    pub program_written: bool,
    /// Configured safe state.
    pub safe_state: PlcValue,
}

/// Why the effective value was chosen (tests / diagnostics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveSource {
    /// FAULT or global force_safe → safe_state.
    GlobalSafe,
    /// Maintenance force overlay.
    Force,
    /// Module Bad + force_safe policy.
    ModuleSafe,
    /// Program `%Q`.
    Program,
    /// Never written → safe_state.
    DefaultSafe,
}

/// Resolve effective field output per architecture priority table.
#[must_use]
pub fn resolve_effective_output(input: EffectiveOutputInput) -> (PlcValue, EffectiveSource) {
    // 1. FAULT or global force_safe
    if input.global_force_safe || input.mode == OperatingMode::Fault {
        return (input.safe_state, EffectiveSource::GlobalSafe);
    }
    // 2. Maintenance force overlay
    if let Some(f) = input.force {
        return (f.value, EffectiveSource::Force);
    }
    // 3. Module quality Bad and force_safe policy
    if input.module_quality.is_bad() && input.on_bad_quality == BadQualityPolicy::ForceSafe {
        return (input.safe_state, EffectiveSource::ModuleSafe);
    }
    // 4. Program %Q
    if input.program_written {
        return (input.program_value, EffectiveSource::Program);
    }
    // 5. Never written → safe_state
    (input.safe_state, EffectiveSource::DefaultSafe)
}

#[cfg(test)]
mod tests {
    use super::*;
    use plc_types::OperatingMode;

    fn base() -> EffectiveOutputInput {
        EffectiveOutputInput {
            mode: OperatingMode::Run,
            global_force_safe: false,
            module_quality: Quality::Good,
            on_bad_quality: BadQualityPolicy::ForceSafe,
            force: None,
            program_value: PlcValue::Bool(true),
            program_written: true,
            safe_state: PlcValue::Bool(false),
        }
    }

    #[test]
    fn priority_global_safe_beats_force() {
        let mut i = base();
        i.global_force_safe = true;
        i.force = Some(ForceOverlay {
            value: PlcValue::Bool(true),
        });
        let (v, src) = resolve_effective_output(i);
        assert_eq!(v, PlcValue::Bool(false));
        assert_eq!(src, EffectiveSource::GlobalSafe);
    }

    #[test]
    fn priority_fault_mode() {
        let mut i = base();
        i.mode = OperatingMode::Fault;
        let (v, src) = resolve_effective_output(i);
        assert_eq!(v, PlcValue::Bool(false));
        assert_eq!(src, EffectiveSource::GlobalSafe);
    }

    #[test]
    fn priority_force_over_program() {
        let mut i = base();
        i.force = Some(ForceOverlay {
            value: PlcValue::Bool(false),
        });
        i.program_value = PlcValue::Bool(true);
        let (v, src) = resolve_effective_output(i);
        assert_eq!(v, PlcValue::Bool(false));
        assert_eq!(src, EffectiveSource::Force);
    }

    #[test]
    fn priority_module_bad_force_safe() {
        let mut i = base();
        i.module_quality = Quality::Bad;
        i.program_value = PlcValue::Bool(true);
        let (v, src) = resolve_effective_output(i);
        assert_eq!(v, PlcValue::Bool(false));
        assert_eq!(src, EffectiveSource::ModuleSafe);
    }

    #[test]
    fn priority_module_bad_hold_last() {
        let mut i = base();
        i.module_quality = Quality::Bad;
        i.on_bad_quality = BadQualityPolicy::HoldLast;
        i.program_value = PlcValue::Bool(true);
        let (v, src) = resolve_effective_output(i);
        assert_eq!(v, PlcValue::Bool(true));
        assert_eq!(src, EffectiveSource::Program);
    }

    #[test]
    fn priority_program_over_default() {
        let i = base();
        let (v, src) = resolve_effective_output(i);
        assert_eq!(v, PlcValue::Bool(true));
        assert_eq!(src, EffectiveSource::Program);
    }

    #[test]
    fn priority_never_written() {
        let mut i = base();
        i.program_written = false;
        let (v, src) = resolve_effective_output(i);
        assert_eq!(v, PlcValue::Bool(false));
        assert_eq!(src, EffectiveSource::DefaultSafe);
    }

    #[test]
    fn force_table_clear_all() {
        let mut t = ForceTable::new();
        t.set(0, PlcValue::Bool(true));
        t.set(1, PlcValue::Real(1.0));
        assert!(!t.is_empty());
        t.clear_all();
        assert!(t.is_empty());
    }
}
