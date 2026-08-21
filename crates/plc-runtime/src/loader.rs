//! Upload / validate / arm / activate.

use plc_fb_primitives::PRIMITIVE_ABI;
use plc_ir::RetainLayout;
use plc_package::{validate, Manifest, ParsedPackage, RestartPolicy, VerifyPolicy, VerifyingKey};
use plc_retain::{map_retain, MapReport};
use plc_scan::{
    ActivateRequest, ArmedProgram, OutputRestartPolicy, RetainCopy, ScanClock, ScanEngine, ScanIo,
    ScanPlan, StepOutcome,
};
use plc_types::{OperatingMode, ProgramPhase};
use plc_vm::{Vm, VmConfig};

use crate::error::RuntimeError;

/// Options for [`Runtime::new`].
#[derive(Debug, Clone, Default)]
pub struct RuntimeConfig {
    /// When true, unsigned / bad signatures fail arm (still not FAULT).
    pub require_signature: bool,
    /// Trust anchors for Ed25519 verify.
    pub public_keys: Vec<VerifyingKey>,
    /// Zero incompatible retain slots instead of rejecting arm.
    pub force_retain_incompat: bool,
}

/// Result of a successful [`Runtime::upload`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmReport {
    /// Armed program id.
    pub program_id: String,
    /// Manifest compatibility hash.
    pub compatibility_hash: String,
    /// Keep / drop / cold / force-zeroed counts.
    pub retain: MapReport,
    /// Manifest restart policy (before eligibility).
    pub restart_policy: RestartPolicy,
    /// True when `bumpless` was honored (`compatibility_hash` matched current).
    pub bumpless_honored: bool,
    /// True when the operator asked for bumpless but hash did not match.
    pub bumpless_downgraded: bool,
}

/// Dual-buffer loader in front of a [`ScanEngine`].
pub struct Runtime {
    engine: ScanEngine,
    config: RuntimeConfig,
    current_layout: Option<RetainLayout>,
    armed_layout: Option<RetainLayout>,
    last_arm: Option<ArmReport>,
}

impl Runtime {
    /// Construct an engine with no program loaded (`phase=idle`, `mode=STOP`).
    pub fn new(
        plan: ScanPlan,
        io: ScanIo,
        clock: Box<dyn ScanClock>,
        config: RuntimeConfig,
    ) -> Result<Self, RuntimeError> {
        Ok(Self {
            engine: ScanEngine::new(plan, io, None, clock)?,
            config,
            current_layout: None,
            armed_layout: None,
            last_arm: None,
        })
    }

    /// Scan engine (tests / status).
    #[must_use]
    pub fn engine(&self) -> &ScanEngine {
        &self.engine
    }

    /// Mutable scan engine (mode, inject, VM inspection).
    ///
    /// Do not call [`ScanEngine::step`] / [`ScanEngine::run_due`] through this
    /// handle: those skip retain-layout promotion and are unsafe for retain
    /// policy. After activate, use [`Self::step`] / [`Self::run_due`] so
    /// `current_layout` tracks buffer A.
    pub fn engine_mut(&mut self) -> &mut ScanEngine {
        &mut self.engine
    }

    /// Operator mode.
    #[must_use]
    pub fn mode(&self) -> OperatingMode {
        self.engine.mode()
    }

    /// Program phase.
    #[must_use]
    pub fn phase(&self) -> ProgramPhase {
        self.engine.epoch_hooks().phase()
    }

    /// Last successful arm report.
    #[must_use]
    pub fn last_arm(&self) -> Option<&ArmReport> {
        self.last_arm.as_ref()
    }

    /// Validate `bytes` and arm buffer B. Failures leave the current program
    /// and mode untouched and **never** enter FAULT.
    ///
    /// Only a *successful* upload replaces buffer B (KD-4a rule 5). A failed
    /// re-upload keeps the previously armed package, `last_arm`, and phase.
    pub fn upload(&mut self, bytes: &[u8]) -> Result<ArmReport, RuntimeError> {
        if self.phase() == ProgramPhase::Swapping {
            return Err(RuntimeError::conflict("upload while swapping"));
        }
        // Promote layouts if a prior activate completed via `engine_mut().step()`.
        self.sync_layouts_after_step();
        self.engine
            .epoch_hooks()
            .set_phase(ProgramPhase::Validating);

        match self.arm_validated(bytes) {
            Ok(report) => Ok(report),
            Err(e) => {
                let phase = if self.engine.armed_program_id().is_some() {
                    ProgramPhase::Armed
                } else {
                    ProgramPhase::Idle
                };
                self.engine.epoch_hooks().set_phase(phase);
                Err(e)
            }
        }
    }

    fn arm_validated(&mut self, bytes: &[u8]) -> Result<ArmReport, RuntimeError> {
        let policy = if self.config.require_signature {
            VerifyPolicy::required(&self.config.public_keys)
        } else {
            VerifyPolicy::unsigned()
        };
        let parsed = validate(bytes, policy)?;
        self.build_and_arm(parsed)
    }

    fn build_and_arm(&mut self, parsed: ParsedPackage) -> Result<ArmReport, RuntimeError> {
        let manifest = &parsed.manifest;
        if manifest.primitive_abi != PRIMITIVE_ABI {
            return Err(RuntimeError::arm(format!(
                "primitive_abi {} != runtime {PRIMITIVE_ABI}",
                manifest.primitive_abi
            )));
        }
        let module = parsed
            .modules
            .into_iter()
            .next()
            .ok_or_else(|| RuntimeError::arm("validated package has no spbc module"))?;

        let mut task_entries = Vec::with_capacity(self.engine.plan().tasks.len());
        for task in &self.engine.plan().tasks {
            let Some(symbol) = manifest.task_entries.get(&task.name) else {
                return Err(RuntimeError::arm(format!(
                    "package missing task_entries for '{}'",
                    task.name
                )));
            };
            task_entries.push(symbol.clone());
        }

        let mut vm = Vm::load(module, &VmConfig::default())?;
        let new_layout = manifest.retain_layout(u32::try_from(vm.retain().len()).unwrap_or(0))?;

        let (report, copies) = self.install_shadow_retain(&mut vm, &new_layout)?;
        let (restart_policy, bumpless_honored, bumpless_downgraded) =
            resolve_restart(manifest, self.engine.current_compatibility_hash());

        self.engine.arm(ArmedProgram {
            vm,
            program_id: manifest.id.clone(),
            compatibility_hash: manifest.compatibility_hash.clone(),
            task_entries,
            restart_policy,
            retain_copies: copies,
        })?;
        self.armed_layout = Some(new_layout);

        let report = ArmReport {
            program_id: manifest.id.clone(),
            compatibility_hash: manifest.compatibility_hash.clone(),
            retain: report,
            restart_policy: manifest.restart_policy,
            bumpless_honored,
            bumpless_downgraded,
        };
        self.last_arm = Some(report.clone());
        Ok(report)
    }

    fn install_shadow_retain(
        &self,
        vm: &mut Vm,
        new_layout: &RetainLayout,
    ) -> Result<(MapReport, Vec<RetainCopy>), RuntimeError> {
        let Some(old_layout) = self.current_layout.as_ref() else {
            return Ok((MapReport::default(), Vec::new()));
        };
        let Some(cur) = self.engine.vm() else {
            return Ok((MapReport::default(), Vec::new()));
        };
        let mapped = map_retain(
            old_layout,
            cur.retain().as_bytes(),
            new_layout,
            self.config.force_retain_incompat,
        )?;
        vm.load_retain_image(&mapped.image, new_layout)?;
        let mut copies = Vec::new();
        for new_sym in &new_layout.symbols {
            if let Some(old) = old_layout.get(&new_sym.name) {
                if old.ty == new_sym.ty {
                    copies.push(RetainCopy {
                        src_offset: old.offset as usize,
                        dst_offset: new_sym.offset as usize,
                        len: old.ty.byte_width(),
                    });
                }
            }
        }
        Ok((mapped.report, copies))
    }

    /// Drop buffer B.
    pub fn disarm(&mut self) -> Result<(), RuntimeError> {
        self.engine.disarm()?;
        self.armed_layout = None;
        self.last_arm = None;
        Ok(())
    }

    /// Request activate (swap runs at the next highest-priority quiet boundary).
    pub fn activate(&mut self) -> Result<ActivateRequest, RuntimeError> {
        let req = self.engine.request_activate()?;
        if req == ActivateRequest::NoOp {
            self.armed_layout = None;
            self.last_arm = None;
        }
        Ok(req)
    }

    /// One cooperative step (may complete an armed activate).
    pub fn step(&mut self) -> Result<StepOutcome, RuntimeError> {
        let out = self.engine.step()?;
        self.sync_layouts_after_step();
        Ok(out)
    }

    /// Run every due task.
    pub fn run_due(&mut self) -> Result<u32, RuntimeError> {
        let n = self.engine.run_due()?;
        self.sync_layouts_after_step();
        Ok(n)
    }

    fn sync_layouts_after_step(&mut self) {
        if self.engine.armed_program_id().is_none() {
            if self.engine.current_program_id().is_some() && self.armed_layout.is_some() {
                // Arm is gone because commit swung B → current; promote that layout.
                self.current_layout = self.armed_layout.take();
            } else {
                // Disarm / activate NoOp dropped B without committing it.
                self.armed_layout = None;
            }
        }
    }
}

fn resolve_restart(
    manifest: &Manifest,
    current_hash: Option<&str>,
) -> (OutputRestartPolicy, bool, bool) {
    match manifest.restart_policy {
        RestartPolicy::SafeReset => (OutputRestartPolicy::SafeReset, false, false),
        RestartPolicy::Bumpless => {
            if current_hash == Some(manifest.compatibility_hash.as_str()) {
                (OutputRestartPolicy::Bumpless, true, false)
            } else {
                (OutputRestartPolicy::SafeReset, false, true)
            }
        }
    }
}
