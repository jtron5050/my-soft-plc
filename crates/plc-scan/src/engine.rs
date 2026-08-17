//! Cooperative scan engine: schedule + I → L → Q.

use std::sync::Arc;

use plc_config::{DeviceConfig, StopOutputPolicy};
use plc_io::{
    resolve_effective_output, BadQualityPolicy, DoubleBuffer, EffectiveOutputInput, ForceTable,
    InputUpdate, IoDriver, OutputImage, ProcessImage,
};
use plc_types::{OperatingMode, Quality};
use plc_vm::Vm;

use crate::clock::{MonotonicClock, ScanClock};
use crate::convert::{plc_to_vm, vm_to_plc};
use crate::error::ScanError;
use crate::hooks::EpochHooks;
use crate::mode::{ModeCell, ModeRequest, ScanHandle};
use crate::retain_signal::{RetainDirtySignal, RetainDirtyWatch};
use crate::status::{ScanStatusSnapshot, TaskTiming};
use crate::telemetry::{telemetry_channel, PublishTrack, TelemetrySink, TelemetrySource};
use crate::watchdog::{HardwareWatchdog, NullWatchdog, SoftwareWatchdog};

/// Default telemetry ring depth.
pub const DEFAULT_TELEMETRY_CAPACITY: usize = 1024;

/// One cooperative cyclic task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskPlan {
    /// Task name (`fast`, `main`, `slow`).
    pub name: String,
    /// Period in milliseconds.
    pub period_ms: u32,
    /// IR entry symbol.
    pub entry: String,
    /// Higher runs first when several tasks are due.
    pub priority: u8,
}

/// Owned scan configuration (cold-path copy of device config).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanPlan {
    /// Cooperative task table.
    pub tasks: Vec<TaskPlan>,
    /// Consecutive overruns before FAULT.
    pub overrun_limit: u32,
    /// STOP output policy.
    pub stop_output_policy: StopOutputPolicy,
    /// Enter FAULT when module quality is Bad.
    pub fault_on_module_bad: bool,
    /// Master telemetry enable.
    pub telemetry_enabled: bool,
    /// Analog re-publish period.
    pub analog_period_ms: u32,
    /// Digital CoS minimum period.
    pub digital_cos_ms: u32,
    /// Telemetry ring capacity.
    pub telemetry_capacity: usize,
}

impl ScanPlan {
    /// Build a plan from tasks with architecture defaults.
    pub fn new(tasks: Vec<TaskPlan>) -> Result<Self, ScanError> {
        validate_tasks(&tasks)?;
        Ok(Self {
            tasks,
            overrun_limit: 2,
            stop_output_policy: StopOutputPolicy::Safe,
            fault_on_module_bad: false,
            telemetry_enabled: true,
            analog_period_ms: 500,
            digital_cos_ms: 20,
            telemetry_capacity: DEFAULT_TELEMETRY_CAPACITY,
        })
    }

    /// Copy policy + task table from a loaded device config.
    pub fn from_config(cfg: &DeviceConfig) -> Result<Self, ScanError> {
        let tasks: Vec<TaskPlan> = cfg
            .scan
            .tasks
            .iter()
            .map(|t| TaskPlan {
                name: t.name.clone(),
                period_ms: t.period_ms,
                entry: t.entry.clone(),
                priority: t.priority,
            })
            .collect();
        validate_tasks(&tasks)?;
        Ok(Self {
            tasks,
            overrun_limit: cfg.scan.overrun_limit,
            stop_output_policy: cfg.stop_output_policy,
            fault_on_module_bad: cfg.io.fault_on_module_bad,
            telemetry_enabled: cfg.telemetry.enabled,
            analog_period_ms: cfg.telemetry.analog_period_ms,
            digital_cos_ms: cfg.telemetry.digital_cos_ms,
            telemetry_capacity: DEFAULT_TELEMETRY_CAPACITY,
        })
    }
}

fn validate_tasks(tasks: &[TaskPlan]) -> Result<(), ScanError> {
    if tasks.is_empty() {
        return Err(ScanError::config("scan.tasks must not be empty"));
    }
    let mut names = std::collections::BTreeSet::new();
    for t in tasks {
        if t.name.trim().is_empty() {
            return Err(ScanError::config("task.name must be non-empty"));
        }
        if !names.insert(t.name.as_str()) {
            return Err(ScanError::config(format!(
                "duplicate task.name '{}'",
                t.name
            )));
        }
        if t.period_ms == 0 {
            return Err(ScanError::config(format!(
                "task '{}': period_ms must be > 0",
                t.name
            )));
        }
        if t.entry.trim().is_empty() {
            return Err(ScanError::config(format!(
                "task '{}': entry must be non-empty",
                t.name
            )));
        }
    }
    Ok(())
}

/// I/O bundle owned by the engine.
pub struct ScanIo {
    /// Process image (`%I` / `%Q` / `%M`).
    pub image: ProcessImage,
    /// In-RT or sim driver (network drivers stay off this thread).
    pub driver: Box<dyn IoDriver>,
    /// Optional remote input snapshot (non-blocking copy).
    pub remote_inputs: Option<Arc<DoubleBuffer>>,
    /// Maintenance force overlay.
    pub forces: ForceTable,
    /// Aggregated module quality.
    pub module_quality: Quality,
    /// Policy when module quality is Bad.
    pub on_bad_quality: BadQualityPolicy,
}

impl ScanIo {
    /// Bundle an image and driver (no remote buffer).
    #[must_use]
    pub fn new(image: ProcessImage, driver: Box<dyn IoDriver>) -> Self {
        Self {
            image,
            driver,
            remote_inputs: None,
            forces: ForceTable::new(),
            module_quality: Quality::Good,
            on_bad_quality: BadQualityPolicy::ForceSafe,
        }
    }

    /// Mutable driver (tests inject via a shared wrapper).
    pub fn driver_mut(&mut self) -> &mut dyn IoDriver {
        &mut *self.driver
    }
}

/// Result of [`ScanEngine::step`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
    /// No task is due at `now`.
    Idle {
        /// Next wake time (engine clock ms).
        next_due_ms: u64,
    },
    /// One task ran to completion.
    Ran {
        /// Task index in [`ScanPlan::tasks`].
        task: usize,
        /// Invocation duration (including inject).
        duration_us: u64,
    },
}

struct TaskRuntime {
    next_due_ms: u64,
    last_us: u64,
    max_us: u64,
    overruns: u32,
}

/// Cooperative cyclic scan engine.
pub struct ScanEngine {
    plan: ScanPlan,
    io: ScanIo,
    vm: Option<Vm>,
    clock: Box<dyn ScanClock>,
    modes: ModeCell,
    sw_wd: SoftwareWatchdog,
    hw_wd: Box<dyn HardwareWatchdog>,
    tel_sink: TelemetrySink,
    tel_src: TelemetrySource,
    tel_in: Vec<PublishTrack>,
    tel_out: Vec<PublishTrack>,
    retain: RetainDirtySignal,
    hooks: EpochHooks,
    tasks: Vec<TaskRuntime>,
    schedule_order: Vec<usize>,
    input_scratch: InputUpdate,
    output_image: OutputImage,
    first_run_complete: bool,
    min_duration_us: Vec<u64>,
    last_ran: Option<usize>,
}

impl ScanEngine {
    /// Construct from a device config with a monotonic clock.
    pub fn from_config(cfg: &DeviceConfig, io: ScanIo, vm: Option<Vm>) -> Result<Self, ScanError> {
        Self::new(
            ScanPlan::from_config(cfg)?,
            io,
            vm,
            Box::new(MonotonicClock::new()),
        )
    }

    /// Construct with an explicit plan and clock.
    pub fn new(
        plan: ScanPlan,
        mut io: ScanIo,
        vm: Option<Vm>,
        clock: Box<dyn ScanClock>,
    ) -> Result<Self, ScanError> {
        if let Some(ref vm) = vm {
            if vm.inputs().len() != io.image.inputs.len() {
                return Err(ScanError::image_mismatch(format!(
                    "VM inputs {} != image inputs {}",
                    vm.inputs().len(),
                    io.image.inputs.len()
                )));
            }
            if vm.outputs().len() != io.image.outputs.len() {
                return Err(ScanError::image_mismatch(format!(
                    "VM outputs {} != image outputs {}",
                    vm.outputs().len(),
                    io.image.outputs.len()
                )));
            }
        }

        io.driver.start()?;

        let n = plan.tasks.len();
        let n_i = io.image.inputs.len();
        let n_q = io.image.outputs.len();
        let mut schedule_order: Vec<usize> = (0..n).collect();
        schedule_order.sort_by(|&a, &b| {
            plan.tasks[b]
                .priority
                .cmp(&plan.tasks[a].priority)
                .then_with(|| plan.tasks[a].name.cmp(&plan.tasks[b].name))
        });

        let mut input_scratch = InputUpdate::zeros(n_i);
        input_scratch.values.reserve(n_i);
        input_scratch.quality.reserve(n_i);

        let mut output_values = Vec::with_capacity(n_q);
        output_values.resize(n_q, plc_io::PlcValue::Bool(false));

        let cap = plan.telemetry_capacity.max(1);
        let (tel_sink, tel_src) = telemetry_channel(cap);

        Ok(Self {
            sw_wd: SoftwareWatchdog::new(n, plan.overrun_limit),
            hooks: EpochHooks::new(n),
            min_duration_us: vec![0; n],
            tasks: (0..n)
                .map(|_| TaskRuntime {
                    next_due_ms: 0,
                    last_us: 0,
                    max_us: 0,
                    overruns: 0,
                })
                .collect(),
            schedule_order,
            tel_in: vec![PublishTrack::new(); n_i],
            tel_out: vec![PublishTrack::new(); n_q],
            plan,
            io,
            vm,
            clock,
            modes: ModeCell::new(),
            hw_wd: Box::new(NullWatchdog),
            tel_sink,
            tel_src,
            retain: RetainDirtySignal::new(),
            input_scratch,
            output_image: OutputImage {
                values: output_values,
                force_safe: true,
            },
            first_run_complete: false,
            last_ran: None,
        })
    }

    /// Replace the hardware-watchdog stub (tests).
    #[must_use]
    pub fn with_hw_watchdog(mut self, wd: Box<dyn HardwareWatchdog>) -> Self {
        self.hw_wd = wd;
        self
    }

    /// Non-RT mode handle.
    #[must_use]
    pub fn handle(&self) -> ScanHandle {
        ScanHandle::new(self.modes.clone())
    }

    /// Queue a mode request (same as [`ScanHandle::request_mode`]).
    pub fn request_mode(&self, req: ModeRequest) {
        self.modes.request(req);
    }

    /// Current operator mode.
    #[must_use]
    pub fn mode(&self) -> OperatingMode {
        self.modes.mode()
    }

    /// Status snapshot.
    #[must_use]
    pub fn status(&self) -> ScanStatusSnapshot {
        ScanStatusSnapshot {
            mode: self.mode(),
            phase: self.hooks.phase(),
            tasks: self
                .plan
                .tasks
                .iter()
                .enumerate()
                .map(|(i, t)| TaskTiming {
                    name: t.name.clone(),
                    period_ms: t.period_ms,
                    last_us: self.tasks[i].last_us,
                    max_us: self.tasks[i].max_us,
                    overruns: self.tasks[i].overruns,
                    consecutive_overruns: self.sw_wd.consecutive(i),
                })
                .collect(),
            telemetry_drops: self.tel_src.drops(),
            mode_rejected: self.modes.rejected(),
            io_degraded: self.io.module_quality.is_bad(),
            first_run_complete: self.first_run_complete,
        }
    }

    /// Telemetry consumer (cloneable).
    #[must_use]
    pub fn telemetry_source(&self) -> TelemetrySource {
        self.tel_src.clone()
    }

    /// Retain-dirty consumer.
    #[must_use]
    pub fn retain_dirty(&self) -> RetainDirtyWatch {
        self.retain.watch()
    }

    /// Epoch hooks (PR-10).
    #[must_use]
    pub fn epoch_hooks(&self) -> EpochHooks {
        self.hooks.clone()
    }

    /// Task table.
    #[must_use]
    pub fn plan(&self) -> &ScanPlan {
        &self.plan
    }

    /// Process image + driver (tests).
    pub fn io_mut(&mut self) -> &mut ScanIo {
        &mut self.io
    }

    /// Armed VM, if any.
    #[must_use]
    pub fn vm(&self) -> Option<&Vm> {
        self.vm.as_ref()
    }

    /// Mutable VM (tests / retain inspection).
    pub fn vm_mut(&mut self) -> Option<&mut Vm> {
        self.vm.as_mut()
    }

    /// Last task index that completed an invocation.
    #[must_use]
    pub fn last_ran(&self) -> Option<usize> {
        self.last_ran
    }

    /// Inject a minimum invocation duration for overrun tests (`0` / `None` clears).
    pub fn set_min_duration_us(&mut self, task: &str, us: Option<u64>) -> Result<(), ScanError> {
        let idx = self
            .plan
            .tasks
            .iter()
            .position(|t| t.name == task)
            .ok_or_else(|| ScanError::config(format!("unknown task '{task}'")))?;
        self.min_duration_us[idx] = us.unwrap_or(0);
        Ok(())
    }

    /// Run every task that is due, highest priority first. Returns how many ran.
    pub fn run_due(&mut self) -> Result<u32, ScanError> {
        self.apply_mode_boundary();
        let mut n = 0u32;
        loop {
            let now = self.clock.now_ms();
            let Some(task) = self.pick_due(now) else {
                break;
            };
            self.invoke(task);
            n = n.saturating_add(1);
        }
        Ok(n)
    }

    /// Run the highest-priority due task, or return Idle.
    pub fn step(&mut self) -> Result<StepOutcome, ScanError> {
        self.apply_mode_boundary();
        let now = self.clock.now_ms();
        let Some(task) = self.pick_due(now) else {
            return Ok(StepOutcome::Idle {
                next_due_ms: self.next_wakeup_ms(),
            });
        };
        let duration_us = self.invoke(task);
        Ok(StepOutcome::Ran { task, duration_us })
    }

    fn apply_mode_boundary(&mut self) {
        match self.modes.apply_pending() {
            Ok((_mode, clear_forces)) => {
                if clear_forces {
                    self.io.forces.clear_all();
                }
            }
            Err(_) => {
                // Counted on the cell; keep running in the current mode.
            }
        }
    }

    fn pick_due(&self, now_ms: u64) -> Option<usize> {
        self.schedule_order
            .iter()
            .copied()
            .find(|&i| self.tasks[i].next_due_ms <= now_ms)
    }

    fn next_wakeup_ms(&self) -> u64 {
        self.tasks.iter().map(|t| t.next_due_ms).min().unwrap_or(0)
    }

    fn enter_fault(&mut self) {
        self.modes.set_mode(OperatingMode::Fault);
        self.io.forces.clear_all();
    }

    fn invoke(&mut self, task: usize) -> u64 {
        self.hooks.set_in_invocation(true);
        let t0 = self.clock.now_ns();
        let now_ms = t0 / 1_000_000;
        let period_ms = self.plan.tasks[task].period_ms;

        self.sample_inputs();

        let mode = self.mode();
        let mut ran_logic_ok = false;
        if mode.executes_logic() {
            match self.run_logic(task, now_ms) {
                Ok(()) => ran_logic_ok = true,
                Err(_) => {
                    self.enter_fault();
                }
            }
        }

        let mut duration_us = self.clock.now_ns().saturating_sub(t0) / 1000;
        let inject = self.min_duration_us.get(task).copied().unwrap_or(0);
        if inject > duration_us {
            duration_us = inject;
        }

        // Logic-overrun FAULT only applies while executing (RUN/SIM), not STOP/FAULT ticks.
        if mode.executes_logic() {
            let trip = self.sw_wd.note(task, duration_us, period_ms);
            if duration_us >= u64::from(period_ms).saturating_mul(1000) {
                self.tasks[task].overruns = self.tasks[task].overruns.saturating_add(1);
            }
            if trip {
                self.enter_fault();
            }
        }

        self.tasks[task].last_us = duration_us;
        if duration_us > self.tasks[task].max_us {
            self.tasks[task].max_us = duration_us;
        }

        self.apply_outputs(ran_logic_ok);
        if ran_logic_ok {
            self.first_run_complete = true;
        }

        if self.plan.telemetry_enabled {
            self.publish_telemetry(now_ms);
        }

        self.hw_wd.stroke();

        if self.hooks.first_scan(task) {
            self.hooks.clear_first_scan(task);
        }

        let period = u64::from(period_ms);
        self.tasks[task].next_due_ms = self.tasks[task].next_due_ms.saturating_add(period);
        self.last_ran = Some(task);
        self.hooks.set_in_invocation(false);
        debug_assert!(self.hooks.is_quiet());
        duration_us
    }

    fn sample_inputs(&mut self) {
        if let Some(db) = self.io.remote_inputs.clone() {
            let snap = db.read(8);
            for (i, (v, q)) in snap.values.iter().zip(snap.quality.iter()).enumerate() {
                if i < self.io.image.inputs.len() {
                    let _ = self.io.image.set_input(i, *v, *q);
                }
            }
        }

        match self.io.driver.poll_inputs(&mut self.input_scratch) {
            Ok(()) => {
                let n = self.io.image.inputs.len();
                for i in 0..n {
                    let v = self
                        .input_scratch
                        .values
                        .get(i)
                        .copied()
                        .unwrap_or(plc_io::PlcValue::Bool(false));
                    let q = self
                        .input_scratch
                        .quality
                        .get(i)
                        .copied()
                        .unwrap_or(Quality::Good);
                    let _ = self.io.image.set_input(i, v, q);
                }
            }
            Err(_) => {
                self.io.module_quality = Quality::Bad;
                if self.plan.fault_on_module_bad {
                    self.enter_fault();
                }
            }
        }

        let mut worst = Quality::Good;
        for slot in &self.io.image.inputs {
            if slot.quality.is_bad() {
                worst = Quality::Bad;
                break;
            }
            if slot.quality == Quality::Uncertain {
                worst = Quality::Uncertain;
            }
        }
        self.io.module_quality = worst;
        if worst.is_bad() && self.plan.fault_on_module_bad {
            self.enter_fault();
        }

        if let Some(vm) = self.vm.as_mut() {
            for (i, slot) in self.io.image.inputs.iter().enumerate() {
                let _ = vm.inputs_mut().set(i, plc_to_vm(slot.value), 0);
                let _ = vm
                    .inputs_mut()
                    .set_quality_good(i, slot.quality.is_good(), 0);
            }
        }
    }

    fn run_logic(&mut self, task: usize, now_ms: u64) -> Result<(), ScanError> {
        let Some(vm) = self.vm.as_mut() else {
            return Err(ScanError::invalid_state("RUN/SIM with no program armed"));
        };
        let entry = self.plan.tasks[task].entry.as_str();
        vm.run_entry(entry, now_ms)?;
        if vm.retain_dirty {
            self.retain.notify();
            vm.clear_retain_dirty();
        }
        for i in 0..vm.outputs().len() {
            if let Ok(v) = vm.outputs().get(i, 0) {
                let _ = self.io.image.set_output(i, vm_to_plc(v));
            }
        }
        Ok(())
    }

    fn apply_outputs(&mut self, ran_logic_ok: bool) {
        let mode = self.mode();
        let startup_hold = !self.first_run_complete && !ran_logic_ok;
        let stop_safe =
            mode == OperatingMode::Stop && self.plan.stop_output_policy == StopOutputPolicy::Safe;
        let global_force_safe = mode == OperatingMode::Fault || stop_safe || startup_hold;

        let n_q = self.io.image.outputs.len();
        self.output_image.values.clear();
        for i in 0..n_q {
            let slot = self
                .io
                .image
                .outputs
                .get(i)
                .copied()
                .unwrap_or_else(|| plc_io::TypedSlot::zero(plc_io::ValueType::Bool));
            let safe = self
                .io
                .image
                .output_safe
                .get(i)
                .copied()
                .unwrap_or(plc_io::PlcValue::Bool(false));
            let (v, _) = resolve_effective_output(EffectiveOutputInput {
                mode,
                global_force_safe,
                module_quality: self.io.module_quality,
                on_bad_quality: self.io.on_bad_quality,
                force: self.io.forces.get(i as u32),
                program_value: slot.value,
                program_written: slot.written,
                safe_state: safe,
            });
            self.output_image.values.push(v);
        }
        self.output_image.force_safe = global_force_safe;
        if self.io.driver.apply_outputs(&self.output_image).is_err() {
            self.io.module_quality = Quality::Bad;
            if self.plan.fault_on_module_bad {
                self.enter_fault();
            }
        }
    }

    fn publish_telemetry(&mut self, now_ms: u64) {
        let analog = self.plan.analog_period_ms;
        let digital = self.plan.digital_cos_ms;
        for (i, slot) in self.io.image.inputs.iter().enumerate() {
            let is_bool = matches!(slot.value, plc_io::PlcValue::Bool(_));
            if self.tel_in[i].should_publish(slot.value, now_ms, is_bool, analog, digital) {
                self.tel_sink.publish(crate::telemetry::TelemetrySample {
                    alias: i as u32,
                    tag_hint: i as u32,
                    value: slot.value,
                    quality: slot.quality,
                    forced: false,
                    now_ms,
                    is_input: true,
                });
                self.tel_in[i].mark(slot.value, now_ms);
            }
        }
        let n_i = self.io.image.inputs.len();
        for (i, slot) in self.io.image.outputs.iter().enumerate() {
            let is_bool = matches!(slot.value, plc_io::PlcValue::Bool(_));
            if self.tel_out[i].should_publish(slot.value, now_ms, is_bool, analog, digital) {
                let forced = self.io.forces.get(i as u32).is_some();
                self.tel_sink.publish(crate::telemetry::TelemetrySample {
                    alias: (n_i + i) as u32,
                    tag_hint: i as u32,
                    value: slot.value,
                    quality: slot.quality,
                    forced,
                    now_ms,
                    is_input: false,
                });
                self.tel_out[i].mark(slot.value, now_ms);
            }
        }
    }
}

impl Drop for ScanEngine {
    fn drop(&mut self) {
        self.io.driver.stop();
    }
}
