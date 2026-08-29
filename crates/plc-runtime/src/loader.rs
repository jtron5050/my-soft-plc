//! Upload / validate / arm / activate.

use plc_fb_primitives::PRIMITIVE_ABI;
use plc_io::{PlcValue, ValueType};
use plc_ir::{IrType, RetainLayout};
use plc_package::{
    validate, Manifest, ParsedPackage, RestartPolicy, TagEntry, TagKind, VerifyPolicy, VerifyingKey,
};
use plc_retain::{map_retain, MapReport};
use plc_scan::{
    ActivateRequest, ArmedProgram, OutputRestartPolicy, RetainCopy, ScanClock, ScanEngine, ScanIo,
    ScanPlan, StepOutcome,
};
use plc_types::{OperatingMode, ProgramPhase, Quality};
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

/// Result of a successful [`Runtime::upload`] / [`Runtime::commit_arm`].
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

/// Operator-visible program identity for REST `/status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramInfo {
    /// Manifest `id`.
    pub id: String,
    /// Manifest semver.
    pub version: String,
    /// Manifest `build_id`.
    pub build_id: String,
    /// Manifest `compatibility_hash`.
    pub compatibility_hash: String,
    /// True when the package carried a non-zero Ed25519 signature.
    pub signed: bool,
    /// Manifest restart policy (before eligibility).
    pub restart_policy: RestartPolicy,
    /// True when bumpless was requested and the current hash matched.
    pub bumpless_eligible: bool,
}

/// Retain bytes copied under a brief lock for off-lock [`Runtime::prepare_arm`].
#[derive(Debug, Clone)]
pub struct RetainSnapshot {
    /// Current retain layout (buffer A).
    pub layout: RetainLayout,
    /// Packed retain image.
    pub bytes: Vec<u8>,
    /// Current program `compatibility_hash`.
    pub current_hash: Option<String>,
}

/// Inputs captured under a brief lock so validate/VM-load can run unlocked.
#[derive(Debug, Clone)]
pub struct ArmContext {
    /// Scan task names in plan order (need matching `task_entries`).
    pub task_names: Vec<String>,
    /// Current retain snapshot, if a program is already current.
    pub retain: Option<RetainSnapshot>,
    /// Current program hash (for bumpless eligibility).
    pub current_hash: Option<String>,
    /// Copy of runtime arm policy.
    pub config: RuntimeConfig,
}

/// Validated buffer-B payload ready for [`Runtime::commit_arm`].
pub struct PreparedArm {
    program: ArmedProgram,
    layout: RetainLayout,
    report: ArmReport,
    info: ProgramInfo,
    tags: Vec<TagEntry>,
}

/// Point-in-time tag read for REST debug.
#[derive(Debug, Clone, PartialEq)]
pub struct TagView {
    /// Dictionary / image name.
    pub name: String,
    /// IR type.
    pub ty: IrType,
    /// Image region.
    pub kind: TagKind,
    /// Current value (image slot).
    pub value: PlcValue,
    /// Slot quality.
    pub quality: Quality,
    /// Maintenance force overlay is active (`%Q` only).
    pub forced: bool,
}

impl TagView {
    /// IEC type name (`BOOL`, …).
    #[must_use]
    pub const fn type_name(&self) -> &'static str {
        match self.ty {
            IrType::Bool => "BOOL",
            IrType::Int => "INT",
            IrType::Dint => "DINT",
            IrType::Real => "REAL",
            IrType::Time => "TIME",
            IrType::Lint => "LINT",
        }
    }
}

/// Dual-buffer loader in front of a [`ScanEngine`].
pub struct Runtime {
    engine: ScanEngine,
    config: RuntimeConfig,
    current_layout: Option<RetainLayout>,
    armed_layout: Option<RetainLayout>,
    last_arm: Option<ArmReport>,
    current_info: Option<ProgramInfo>,
    armed_info: Option<ProgramInfo>,
    current_tags: Vec<TagEntry>,
    armed_tags: Vec<TagEntry>,
    last_uploader: Option<String>,
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
            current_info: None,
            armed_info: None,
            current_tags: Vec::new(),
            armed_tags: Vec::new(),
            last_uploader: None,
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

    /// Current program metadata (after a successful activate).
    #[must_use]
    pub fn current_info(&self) -> Option<&ProgramInfo> {
        self.current_info.as_ref()
    }

    /// Armed program metadata (after a successful arm).
    #[must_use]
    pub fn armed_info(&self) -> Option<&ProgramInfo> {
        self.armed_info.as_ref()
    }

    /// Tag dictionary of the current program, else armed, else empty.
    #[must_use]
    pub fn tag_dictionary(&self) -> &[TagEntry] {
        if !self.current_tags.is_empty() {
            &self.current_tags
        } else {
            &self.armed_tags
        }
    }

    /// Principal id recorded at last successful arm (dual control).
    #[must_use]
    pub fn last_uploader(&self) -> Option<&str> {
        self.last_uploader.as_deref()
    }

    /// Record who armed buffer B.
    pub fn set_uploader(&mut self, principal_id: impl Into<String>) {
        self.last_uploader = Some(principal_id.into());
    }

    /// Arm policy used by [`Self::prepare_arm`].
    #[must_use]
    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    /// Replace arm policy (config live-reload).
    pub fn set_config(&mut self, config: RuntimeConfig) {
        self.config = config;
    }

    /// Validate `bytes` and arm buffer B. Failures leave the current program
    /// and mode untouched and **never** enter FAULT.
    ///
    /// Only a *successful* upload replaces buffer B (KD-4a rule 5). A failed
    /// re-upload keeps the previously armed package, `last_arm`, and phase.
    pub fn upload(&mut self, bytes: &[u8]) -> Result<ArmReport, RuntimeError> {
        let ctx = self.begin_arm()?;
        match Self::prepare_arm(bytes, &ctx) {
            Ok(prepared) => self.commit_arm(prepared),
            Err(e) => {
                self.abort_arm();
                Err(e)
            }
        }
    }

    /// Mark `phase=validating` and snapshot retain for an unlocked prepare.
    ///
    /// Caller must [`Self::commit_arm`] or [`Self::abort_arm`].
    pub fn begin_arm(&mut self) -> Result<ArmContext, RuntimeError> {
        if self.phase() == ProgramPhase::Swapping {
            return Err(RuntimeError::conflict("upload while swapping"));
        }
        self.sync_layouts_after_step();
        self.engine
            .epoch_hooks()
            .set_phase(ProgramPhase::Validating);
        Ok(ArmContext {
            task_names: self
                .engine
                .plan()
                .tasks
                .iter()
                .map(|t| t.name.clone())
                .collect(),
            retain: self.snapshot_retain(),
            current_hash: self.engine.current_compatibility_hash().map(str::to_string),
            config: self.config.clone(),
        })
    }

    /// Restore `phase` after a failed prepare (never FAULT).
    pub fn abort_arm(&mut self) {
        let phase = if self.engine.armed_program_id().is_some() {
            ProgramPhase::Armed
        } else {
            ProgramPhase::Idle
        };
        self.engine.epoch_hooks().set_phase(phase);
    }

    /// Copy current retain under the caller’s lock. Safe to run while scan steps
    /// if the caller holds `Runtime` exclusively for this snapshot only.
    #[must_use]
    pub fn snapshot_retain(&self) -> Option<RetainSnapshot> {
        let layout = self.current_layout.as_ref()?.clone();
        let bytes = self.engine.vm()?.retain().as_bytes().to_vec();
        Some(RetainSnapshot {
            layout,
            bytes,
            current_hash: self.engine.current_compatibility_hash().map(str::to_string),
        })
    }

    /// Validate, load VM, and build shadow retain **without** mutating the engine.
    pub fn prepare_arm(bytes: &[u8], ctx: &ArmContext) -> Result<PreparedArm, RuntimeError> {
        let policy = if ctx.config.require_signature {
            VerifyPolicy::required(&ctx.config.public_keys)
        } else {
            VerifyPolicy::unsigned()
        };
        let parsed = validate(bytes, policy)?;
        build_prepared(parsed, ctx)
    }

    /// Install a prepared buffer B (short critical section).
    pub fn commit_arm(&mut self, prepared: PreparedArm) -> Result<ArmReport, RuntimeError> {
        if self.phase() == ProgramPhase::Swapping {
            return Err(RuntimeError::conflict("upload while swapping"));
        }
        self.engine.arm(prepared.program)?;
        self.armed_layout = Some(prepared.layout);
        self.armed_info = Some(prepared.info);
        self.armed_tags = prepared.tags;
        self.last_arm = Some(prepared.report.clone());
        Ok(prepared.report)
    }

    /// Drop buffer B.
    pub fn disarm(&mut self) -> Result<(), RuntimeError> {
        self.engine.disarm()?;
        self.armed_layout = None;
        self.last_arm = None;
        self.armed_info = None;
        self.armed_tags.clear();
        self.last_uploader = None;
        Ok(())
    }

    /// Request activate (swap runs at the next highest-priority quiet boundary).
    pub fn activate(&mut self) -> Result<ActivateRequest, RuntimeError> {
        let req = self.engine.request_activate()?;
        if req == ActivateRequest::NoOp {
            self.armed_layout = None;
            self.last_arm = None;
            self.armed_info = None;
            self.armed_tags.clear();
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

    /// Debug-read a tag by dictionary or image-meta name.
    pub fn read_tag(&self, name: &str) -> Result<TagView, RuntimeError> {
        let (kind, slot, ty) = self.lookup_tag(name)?;
        let image = &self.engine.io().image;
        let (value, quality, forced) = match kind {
            TagKind::I => {
                let s = image
                    .get_input(slot as usize)
                    .map_err(|_| RuntimeError::not_found(name))?;
                (s.value, s.quality, false)
            }
            TagKind::Q => {
                let s = image
                    .get_output(slot as usize)
                    .map_err(|_| RuntimeError::not_found(name))?;
                let forced = self.engine.io().forces.get(slot).is_some();
                (s.value, s.quality, forced)
            }
            TagKind::M => {
                let s = image
                    .memory
                    .get(slot as usize)
                    .copied()
                    .ok_or_else(|| RuntimeError::not_found(name))?;
                (s.value, s.quality, false)
            }
            TagKind::R | TagKind::Internal => {
                return Err(RuntimeError::bad_request(format!(
                    "tag '{name}' is not a process-image debug slot"
                )));
            }
        };
        Ok(TagView {
            name: name.to_string(),
            ty,
            kind,
            value,
            quality,
            forced,
        })
    }

    /// Set a maintenance force on a `%Q` tag.
    pub fn force_tag(&mut self, name: &str, value: PlcValue) -> Result<(), RuntimeError> {
        let (kind, slot, ty) = self.lookup_tag(name)?;
        if kind != TagKind::Q {
            return Err(RuntimeError::bad_request(format!(
                "force is only supported on %Q tags (got {kind:?})"
            )));
        }
        if !value_matches(value, ty) {
            return Err(RuntimeError::bad_request(format!(
                "force type mismatch for '{name}'"
            )));
        }
        self.engine.io_mut().forces.set(slot, value);
        Ok(())
    }

    /// Clear one `%Q` force.
    pub fn clear_force(&mut self, name: &str) -> Result<(), RuntimeError> {
        let (kind, slot, _) = self.lookup_tag(name)?;
        if kind != TagKind::Q {
            return Err(RuntimeError::bad_request(format!(
                "force is only supported on %Q tags (got {kind:?})"
            )));
        }
        self.engine.io_mut().forces.clear(slot);
        Ok(())
    }

    fn lookup_tag(&self, name: &str) -> Result<(TagKind, u32, IrType), RuntimeError> {
        for t in self.tag_dictionary() {
            if t.name == name {
                let slot = t.slot.ok_or_else(|| {
                    RuntimeError::bad_request(format!("tag '{name}' has no image slot"))
                })?;
                return Ok((t.kind, slot, t.ty.0));
            }
        }
        let image = &self.engine.io().image;
        for (i, meta) in image.input_meta.iter().enumerate() {
            if meta.tag == name {
                return Ok((TagKind::I, i as u32, value_type_to_ir(meta.ty)));
            }
        }
        for (i, meta) in image.output_meta.iter().enumerate() {
            if meta.tag == name {
                return Ok((TagKind::Q, i as u32, value_type_to_ir(meta.ty)));
            }
        }
        for (i, meta) in image.memory_meta.iter().enumerate() {
            if meta.tag == name {
                return Ok((TagKind::M, i as u32, value_type_to_ir(meta.ty)));
            }
        }
        Err(RuntimeError::not_found(name))
    }

    /// Names exposed by GET /tags (dictionary, else image meta).
    #[must_use]
    pub fn tag_names(&self) -> Vec<TagEntry> {
        let dict = self.tag_dictionary();
        if !dict.is_empty() {
            return dict.to_vec();
        }
        let image = &self.engine.io().image;
        let mut out = Vec::new();
        for (i, meta) in image.input_meta.iter().enumerate() {
            out.push(TagEntry {
                name: meta.tag.clone(),
                ty: plc_package::IrTypeName(value_type_to_ir(meta.ty)),
                kind: TagKind::I,
                slot: Some(i as u32),
            });
        }
        for (i, meta) in image.output_meta.iter().enumerate() {
            out.push(TagEntry {
                name: meta.tag.clone(),
                ty: plc_package::IrTypeName(value_type_to_ir(meta.ty)),
                kind: TagKind::Q,
                slot: Some(i as u32),
            });
        }
        for (i, meta) in image.memory_meta.iter().enumerate() {
            out.push(TagEntry {
                name: meta.tag.clone(),
                ty: plc_package::IrTypeName(value_type_to_ir(meta.ty)),
                kind: TagKind::M,
                slot: Some(i as u32),
            });
        }
        out
    }

    fn sync_layouts_after_step(&mut self) {
        if self.engine.armed_program_id().is_none() {
            if self.engine.current_program_id().is_some() && self.armed_layout.is_some() {
                // Arm is gone because commit swung B → current; promote that layout.
                self.current_layout = self.armed_layout.take();
                self.current_info = self.armed_info.take();
                self.current_tags = std::mem::take(&mut self.armed_tags);
            } else {
                // Disarm / activate NoOp dropped B without committing it.
                self.armed_layout = None;
            }
        }
    }
}

fn build_prepared(parsed: ParsedPackage, ctx: &ArmContext) -> Result<PreparedArm, RuntimeError> {
    let signed = parsed.signature.iter().any(|&b| b != 0);
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

    let mut task_entries = Vec::with_capacity(ctx.task_names.len());
    for name in &ctx.task_names {
        let Some(symbol) = manifest.task_entries.get(name) else {
            return Err(RuntimeError::arm(format!(
                "package missing task_entries for '{name}'"
            )));
        };
        task_entries.push(symbol.clone());
    }

    let mut vm = Vm::load(module, &VmConfig::default())?;
    let new_layout = manifest.retain_layout(u32::try_from(vm.retain().len()).unwrap_or(0))?;

    let (retain_report, copies) = install_shadow_retain(
        &mut vm,
        &new_layout,
        ctx.retain.as_ref(),
        ctx.config.force_retain_incompat,
    )?;
    let current_hash = ctx
        .retain
        .as_ref()
        .and_then(|s| s.current_hash.as_deref())
        .or(ctx.current_hash.as_deref());
    let (restart_policy, bumpless_honored, bumpless_downgraded) =
        resolve_restart(manifest, current_hash);

    let info = ProgramInfo {
        id: manifest.id.clone(),
        version: manifest.version.clone(),
        build_id: manifest.build_id.clone(),
        compatibility_hash: manifest.compatibility_hash.clone(),
        signed,
        restart_policy: manifest.restart_policy,
        bumpless_eligible: bumpless_honored,
    };
    let tags = manifest.tag_dictionary.clone();
    let report = ArmReport {
        program_id: manifest.id.clone(),
        compatibility_hash: manifest.compatibility_hash.clone(),
        retain: retain_report,
        restart_policy: manifest.restart_policy,
        bumpless_honored,
        bumpless_downgraded,
    };
    Ok(PreparedArm {
        program: ArmedProgram {
            vm,
            program_id: manifest.id.clone(),
            compatibility_hash: manifest.compatibility_hash.clone(),
            task_entries,
            restart_policy,
            retain_copies: copies,
        },
        layout: new_layout,
        report,
        info,
        tags,
    })
}

fn install_shadow_retain(
    vm: &mut Vm,
    new_layout: &RetainLayout,
    snapshot: Option<&RetainSnapshot>,
    force_incompat: bool,
) -> Result<(MapReport, Vec<RetainCopy>), RuntimeError> {
    let Some(snap) = snapshot else {
        return Ok((MapReport::default(), Vec::new()));
    };
    let mapped = map_retain(&snap.layout, &snap.bytes, new_layout, force_incompat)?;
    vm.load_retain_image(&mapped.image, new_layout)?;
    let mut copies = Vec::new();
    for new_sym in &new_layout.symbols {
        if let Some(old) = snap.layout.get(&new_sym.name) {
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

fn value_type_to_ir(ty: ValueType) -> IrType {
    match ty {
        ValueType::Bool => IrType::Bool,
        ValueType::Int => IrType::Int,
        ValueType::Dint => IrType::Dint,
        ValueType::Real => IrType::Real,
        ValueType::Time => IrType::Time,
    }
}

fn value_matches(value: PlcValue, ty: IrType) -> bool {
    matches!(
        (value, ty),
        (PlcValue::Bool(_), IrType::Bool)
            | (PlcValue::Int(_), IrType::Int)
            | (PlcValue::Dint(_), IrType::Dint)
            | (PlcValue::Real(_), IrType::Real)
            | (PlcValue::Time(_), IrType::Time)
    )
}
