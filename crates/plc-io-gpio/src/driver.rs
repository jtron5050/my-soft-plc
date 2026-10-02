//! Scan-thread GPIO driver. Hot paths do not allocate.

use std::sync::Arc;

use plc_io::{
    BadQualityPolicy, DriverDiag, FieldGate, InputUpdate, IoDriver, IoError, OutputImage,
    OutputModuleState, PlcValue,
};
use plc_types::{OperatingMode, Quality};

use crate::linux::{config_flags, line_mask, LineClaim, LineIo, LinuxLines};
use crate::validate::{Drive, GpioPlan, LineDirection, PlannedModule};

/// In-RT gpiochip driver.
///
/// `start` requests lines. `poll_inputs` and `apply_outputs` each issue one
/// ioctl per module. SIM writes `safe_state` and does not overlay pin reads.
pub struct GpioDriver {
    modules: Vec<ModuleRt>,
    backend: Option<Box<dyn LineIo>>,
    gate: Arc<FieldGate>,
    fails: u32,
    /// Errno from the latest failed ioctl. Zero after a clean poll or apply.
    last_errno: i32,
    seq: u64,
    running: bool,
}

struct ModuleRt {
    chip_path: std::path::PathBuf,
    offsets: Vec<u32>,
    flags: u64,
    is_input: bool,
    slots: Vec<Option<usize>>,
    safe_bits: u64,
    mask: u64,
    /// Open-drain: process-image 0 is kernel logical 1 (Hi-Z).
    open_drain: bool,
    on_bad: BadQualityPolicy,
    quality: Quality,
    last_bits: u64,
}

impl GpioDriver {
    /// Build the driver. Hardware is claimed in [`IoDriver::start`].
    #[must_use]
    pub fn new(plan: GpioPlan, gate: Arc<FieldGate>) -> Self {
        Self {
            modules: plan
                .modules
                .into_iter()
                .map(ModuleRt::from_planned)
                .collect(),
            backend: None,
            gate,
            fails: 0,
            last_errno: 0,
            seq: 0,
            running: false,
        }
    }
}

impl ModuleRt {
    fn from_planned(module: PlannedModule) -> Self {
        let is_input = module.direction == LineDirection::Input;
        let mut slots = vec![None; module.offsets.len()];
        let mut safe_bits = 0_u64;
        for point in &module.points {
            let bit = usize::from(point.bit);
            if let Some(slot) = slots.get_mut(bit) {
                *slot = Some(point.slot);
            }
            if point.safe {
                safe_bits |= 1_u64 << bit;
            }
        }
        Self {
            flags: config_flags(is_input, module.active_low, module.bias, module.drive),
            mask: line_mask(module.offsets.len()),
            chip_path: module.path,
            offsets: module.offsets,
            is_input,
            slots,
            safe_bits,
            open_drain: module.drive == Some(Drive::OpenDrain),
            on_bad: module.on_bad,
            quality: Quality::Good,
            last_bits: 0,
        }
    }

    fn claim(&self) -> LineClaim {
        LineClaim {
            chip_path: self.chip_path.clone(),
            offsets: self.offsets.clone(),
            flags: self.flags,
            initial_values: kernel_output_bits(self, self.safe_bits),
            is_output: !self.is_input,
            mask: self.mask,
        }
    }
}

impl IoDriver for GpioDriver {
    fn name(&self) -> &'static str {
        "gpio"
    }

    fn start(&mut self) -> Result<(), IoError> {
        if self.running {
            return Ok(());
        }
        let claims: Vec<LineClaim> = self.modules.iter().map(ModuleRt::claim).collect();
        self.backend = Some(Box::new(LinuxLines::claim(&claims)?));
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) {
        let Some(mut backend) = self.backend.take() else {
            self.running = false;
            return;
        };
        self.running = false;
        for (index, module) in self.modules.iter().enumerate() {
            let safe = if module.is_input {
                0
            } else {
                kernel_output_bits(module, module.safe_bits)
            };
            backend.release(index, safe);
        }
    }

    fn poll_inputs(&mut self, out: &mut InputUpdate) -> Result<(), IoError> {
        if !self.running {
            return Err(IoError::NotReady("gpio".into()));
        }
        if self.gate.mode() == OperatingMode::Sim {
            return Ok(());
        }
        let Some(backend) = self.backend.as_mut() else {
            return Err(IoError::NotReady("gpio".into()));
        };
        poll_modules(
            &mut self.modules,
            backend.as_mut(),
            &mut self.fails,
            &mut self.last_errno,
            &mut self.seq,
            out,
        );
        Ok(())
    }

    fn apply_outputs(&mut self, image: &OutputImage) -> Result<(), IoError> {
        if !self.running {
            return Err(IoError::NotReady("gpio".into()));
        }
        let Some(backend) = self.backend.as_mut() else {
            return Err(IoError::NotReady("gpio".into()));
        };
        apply_modules(
            &mut self.modules,
            backend.as_mut(),
            &mut self.fails,
            &mut self.last_errno,
            self.gate.mode() == OperatingMode::Sim,
            image,
        );
        Ok(())
    }

    fn diagnostics(&self) -> DriverDiag {
        DriverDiag {
            status: if !self.running {
                "stopped".into()
            } else if self.last_errno == 0 {
                "running".into()
            } else {
                format!("running errno {}", self.last_errno)
            },
            fail_count: self.fails,
            last_seq: self.seq,
        }
    }

    fn fill_output_module_state(&self, into: &mut [OutputModuleState]) -> bool {
        let mut any = false;
        for module in &self.modules {
            if module.is_input {
                continue;
            }
            for slot in module.slots.iter().flatten() {
                if let Some(state) = into.get_mut(*slot) {
                    state.quality = module.quality;
                    state.on_bad_quality = module.on_bad;
                    any = true;
                }
            }
        }
        any
    }
}

impl Drop for GpioDriver {
    fn drop(&mut self) {
        self.stop();
    }
}

fn poll_modules(
    modules: &mut [ModuleRt],
    backend: &mut dyn LineIo,
    fails: &mut u32,
    last_errno: &mut i32,
    seq: &mut u64,
    out: &mut InputUpdate,
) {
    *seq = seq.wrapping_add(1);
    out.seq = *seq;
    let mut saw_input = false;
    let mut scan_errno = 0_i32;
    for (index, module) in modules.iter_mut().enumerate() {
        if !module.is_input {
            continue;
        }
        saw_input = true;
        match backend.read_bits(index) {
            Ok(bits) => {
                module.quality = Quality::Good;
                module.last_bits = bits;
                write_inputs(module, bits, Quality::Good, out);
            }
            Err(errno) => {
                module.quality = Quality::Bad;
                *fails = fails.saturating_add(1);
                scan_errno = errno;
                write_inputs(module, module.last_bits, Quality::Bad, out);
            }
        }
    }
    if scan_errno != 0 {
        *last_errno = scan_errno;
    } else if saw_input {
        *last_errno = 0;
    }
}

fn write_inputs(module: &ModuleRt, bits: u64, quality: Quality, out: &mut InputUpdate) {
    for (bit, slot) in module.slots.iter().enumerate() {
        let Some(slot) = slot else {
            continue;
        };
        if let Some(dest) = out.values.get_mut(*slot) {
            *dest = PlcValue::Bool(((bits >> bit) & 1) == 1);
        }
        if let Some(dest) = out.quality.get_mut(*slot) {
            *dest = quality;
        }
    }
}

fn apply_modules(
    modules: &mut [ModuleRt],
    backend: &mut dyn LineIo,
    fails: &mut u32,
    last_errno: &mut i32,
    sim_mode: bool,
    image: &OutputImage,
) {
    let mut saw_output = false;
    let mut scan_errno = 0_i32;
    for (index, module) in modules.iter_mut().enumerate() {
        if module.is_input {
            continue;
        }
        saw_output = true;
        // No input read clears Bad on an output module, so keep issuing the set.
        let bits = kernel_output_bits(module, desired_bits(module, image, sim_mode));
        match backend.write_bits(index, bits, module.mask) {
            Ok(()) => module.quality = Quality::Good,
            Err(errno) => {
                module.quality = Quality::Bad;
                *fails = fails.saturating_add(1);
                scan_errno = errno;
            }
        }
    }
    if scan_errno != 0 {
        *last_errno = scan_errno;
    } else if saw_output {
        *last_errno = 0;
    }
}

/// gpiolib open-drain treats logical 1 as Hi-Z and logical 0 as driven low.
/// Process-image false must float, so invert every owned bit.
fn kernel_output_bits(module: &ModuleRt, process_bits: u64) -> u64 {
    if module.open_drain {
        process_bits ^ module.mask
    } else {
        process_bits
    }
}

fn desired_bits(module: &ModuleRt, image: &OutputImage, sim_mode: bool) -> u64 {
    if sim_mode || image.force_safe {
        return module.safe_bits;
    }
    let mut bits = 0_u64;
    for (bit, slot) in module.slots.iter().enumerate() {
        let Some(slot) = slot else {
            continue;
        };
        if image
            .values
            .get(*slot)
            .copied()
            .is_some_and(PlcValue::as_bool)
        {
            bits |= 1_u64 << bit;
        }
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;
    use plc_io::{IoMap, OutputModuleState};
    use std::sync::Mutex;

    struct MockState {
        bits: Vec<u64>,
        fail_read: Vec<bool>,
        fail_write: Vec<bool>,
        reads: Vec<u32>,
        writes: Vec<(usize, u64, u64)>,
        releases: Vec<(usize, u64)>,
    }

    struct MockLines {
        state: Arc<Mutex<MockState>>,
    }

    impl LineIo for MockLines {
        fn read_bits(&mut self, module: usize) -> Result<u64, i32> {
            let mut state = self.state.lock().expect("mock");
            if let Some(count) = state.reads.get_mut(module) {
                *count = count.saturating_add(1);
            }
            if state.fail_read.get(module).copied().unwrap_or(true) {
                return Err(libc::EIO);
            }
            state.bits.get(module).copied().ok_or(libc::EINVAL)
        }

        fn write_bits(&mut self, module: usize, bits: u64, mask: u64) -> Result<(), i32> {
            let mut state = self.state.lock().expect("mock");
            state.writes.push((module, bits, mask));
            if state.fail_write.get(module).copied().unwrap_or(true) {
                return Err(libc::EIO);
            }
            if let Some(slot) = state.bits.get_mut(module) {
                *slot = bits;
            }
            Ok(())
        }

        fn release(&mut self, module: usize, safe_bits: u64) {
            let mut state = self.state.lock().expect("mock");
            state.releases.push((module, safe_bits));
            if let Some(slot) = state.bits.get_mut(module) {
                *slot = safe_bits;
            }
        }
    }

    fn plan_from(yaml_modules: &str) -> GpioPlan {
        let text = format!("version: 1\nmodules:\n{yaml_modules}");
        let map = IoMap::from_yaml_str(&text).expect("map");
        let resolved = map.resolve().expect("resolve");
        crate::validate_map(&map, &resolved).expect("gpio plan")
    }

    fn bench_plan() -> GpioPlan {
        plan_from(
            r#"
  - id: di
    driver: gpio
    config:
      chip: gpiochip0
      lines: [4, 5]
    bindings:
      - tag: Pull
        image: I
        bit: 0
      - tag: Slip
        image: I
        bit: 1
  - id: do_mod
    driver: gpio
    config:
      chip: gpiochip1
      lines: [7, 8]
      drive: push_pull
    on_bad_quality: hold_last
    bindings:
      - tag: Run
        image: Q
        bit: 0
        safe_state: false
      - tag: Alarm
        image: Q
        safe_state: true
"#,
        )
    }

    fn mock_driver(plan: &GpioPlan, gate: Arc<FieldGate>) -> (GpioDriver, Arc<Mutex<MockState>>) {
        let state = Arc::new(Mutex::new(MockState {
            bits: vec![0; plan.modules.len()],
            fail_read: vec![false; plan.modules.len()],
            fail_write: vec![false; plan.modules.len()],
            reads: vec![0; plan.modules.len()],
            writes: Vec::new(),
            releases: Vec::new(),
        }));
        let mut driver = GpioDriver::new(plan.clone(), gate);
        driver.backend = Some(Box::new(MockLines {
            state: Arc::clone(&state),
        }));
        driver.running = true;
        (driver, state)
    }

    #[test]
    fn bit_index_follows_lines_and_one_ioctl_per_module() {
        let plan = bench_plan();
        assert_eq!(plan.modules[0].offsets, vec![4, 5]);
        assert_eq!(plan.modules[1].offsets, vec![7, 8]);
        assert_eq!(plan.modules[1].points[1].bit, 1);
        assert_eq!(plan.modules[1].points[1].offset, 8);
        let gate = Arc::new(FieldGate::new());
        gate.set_mode(OperatingMode::Run);
        let (mut driver, state) = mock_driver(&plan, gate);
        state.lock().expect("mock").bits[0] = 0b10;
        let mut update = InputUpdate::zeros(2);
        let values_cap = update.values.capacity();
        let quality_cap = update.quality.capacity();
        driver.poll_inputs(&mut update).unwrap();
        assert_eq!(update.values[0], PlcValue::Bool(false));
        assert_eq!(update.values[1], PlcValue::Bool(true));
        assert_eq!(update.quality[0], Quality::Good);
        assert_eq!(update.values.capacity(), values_cap);
        assert_eq!(update.quality.capacity(), quality_cap);
        let state_ref = state.lock().expect("mock");
        assert_eq!(state_ref.reads, vec![1, 0]);
        assert!(state_ref.writes.is_empty());
        drop(state_ref);

        driver
            .apply_outputs(&OutputImage {
                values: vec![PlcValue::Bool(true), PlcValue::Bool(true)],
                force_safe: false,
            })
            .unwrap();
        let state_ref = state.lock().expect("mock");
        assert_eq!(state_ref.reads, vec![1, 0]);
        assert_eq!(state_ref.writes, vec![(1, 0b11, 0b11)]);
    }

    #[test]
    fn failed_read_holds_last_value_without_growing() {
        let plan = bench_plan();
        let gate = Arc::new(FieldGate::new());
        gate.set_mode(OperatingMode::Run);
        let (mut driver, state) = mock_driver(&plan, gate);
        state.lock().expect("mock").bits[0] = 0b01;
        let mut update = InputUpdate::zeros(2);
        let cap = update.values.capacity();
        driver.poll_inputs(&mut update).unwrap();
        assert_eq!(update.values[0], PlcValue::Bool(true));
        state.lock().expect("mock").fail_read[0] = true;
        update.values[0] = PlcValue::Bool(false);
        driver.poll_inputs(&mut update).unwrap();
        assert_eq!(update.values[0], PlcValue::Bool(true));
        assert_eq!(update.quality[0], Quality::Bad);
        assert_eq!(update.quality[1], Quality::Bad);
        assert_eq!(update.values.capacity(), cap);
        assert!(driver.diagnostics().fail_count >= 1);
    }

    #[test]
    fn force_safe_and_sim_write_safe_state() {
        let plan = bench_plan();
        let gate = Arc::new(FieldGate::new());
        gate.set_mode(OperatingMode::Run);
        let (mut driver, state) = mock_driver(&plan, Arc::clone(&gate));
        driver
            .apply_outputs(&OutputImage {
                values: vec![PlcValue::Bool(true), PlcValue::Bool(false)],
                force_safe: true,
            })
            .unwrap();
        assert_eq!(
            state.lock().expect("mock").writes.last().copied(),
            Some((1, 0b10, 0b11))
        );

        gate.set_mode(OperatingMode::Sim);
        let mut update = InputUpdate::zeros(2);
        update.values[0] = PlcValue::Bool(true);
        update.quality[0] = Quality::Good;
        state.lock().expect("mock").bits[0] = 0;
        driver.poll_inputs(&mut update).unwrap();
        assert_eq!(update.values[0], PlcValue::Bool(true));
        let writes_before = state.lock().expect("mock").writes.len();
        driver
            .apply_outputs(&OutputImage {
                values: vec![PlcValue::Bool(true), PlcValue::Bool(true)],
                force_safe: false,
            })
            .unwrap();
        let state_ref = state.lock().expect("mock");
        assert_eq!(state_ref.writes.len(), writes_before + 1);
        assert_eq!(state_ref.writes.last().copied(), Some((1, 0b10, 0b11)));
        assert_eq!(state_ref.reads[0], 0);
    }

    #[test]
    fn failed_write_retries_on_the_next_scan() {
        let plan = bench_plan();
        let gate = Arc::new(FieldGate::new());
        gate.set_mode(OperatingMode::Run);
        let (mut driver, state) = mock_driver(&plan, gate);
        state.lock().expect("mock").fail_write[1] = true;
        let image = OutputImage {
            values: vec![PlcValue::Bool(true), PlcValue::Bool(false)],
            force_safe: false,
        };
        driver.apply_outputs(&image).unwrap();
        assert_eq!(state.lock().expect("mock").writes.len(), 1);
        assert!(driver.diagnostics().status.contains("errno"));
        let mut slot_state = vec![
            OutputModuleState {
                quality: Quality::Good,
                on_bad_quality: BadQualityPolicy::ForceSafe,
            };
            2
        ];
        assert!(driver.fill_output_module_state(&mut slot_state));
        assert_eq!(slot_state[0].quality, Quality::Bad);
        assert_eq!(slot_state[0].on_bad_quality, BadQualityPolicy::HoldLast);

        state.lock().expect("mock").fail_write[1] = false;
        let recovered = OutputImage {
            values: vec![PlcValue::Bool(true), PlcValue::Bool(true)],
            force_safe: false,
        };
        driver.apply_outputs(&recovered).unwrap();
        assert!(driver.fill_output_module_state(&mut slot_state));
        assert_eq!(slot_state[0].quality, Quality::Good);
        assert_eq!(driver.diagnostics().status, "running");
        let state_ref = state.lock().expect("mock");
        assert_eq!(state_ref.writes.len(), 2);
        assert_eq!(state_ref.writes[1], (1, 0b11, 0b11));
    }

    #[test]
    fn open_drain_false_floats_and_true_drives_low() {
        let plan = plan_from(
            r#"
  - id: do_mod
    driver: gpio
    config:
      chip: gpiochip1
      lines: [7, 8]
    bindings:
      - tag: Run
        image: Q
        bit: 0
        safe_state: false
      - tag: Alarm
        image: Q
        bit: 1
        safe_state: true
"#,
        );
        assert_eq!(
            plan.modules[0].drive,
            Some(crate::validate::Drive::OpenDrain)
        );
        let gate = Arc::new(FieldGate::new());
        gate.set_mode(OperatingMode::Run);
        let (mut driver, state) = mock_driver(&plan, gate);
        let claim = driver.modules[0].claim();
        assert_eq!(claim.initial_values, 0b01);
        assert_eq!(claim.mask, 0b11);
        driver
            .apply_outputs(&OutputImage {
                values: vec![PlcValue::Bool(true), PlcValue::Bool(false)],
                force_safe: false,
            })
            .unwrap();
        driver
            .apply_outputs(&OutputImage {
                values: vec![PlcValue::Bool(true), PlcValue::Bool(true)],
                force_safe: true,
            })
            .unwrap();
        driver.stop();
        let state_ref = state.lock().expect("mock");
        assert_eq!(state_ref.writes, vec![(0, 0b10, 0b11), (0, 0b01, 0b11)]);
        assert_eq!(state_ref.releases, vec![(0, 0b01)]);
    }

    #[test]
    fn stop_releases_safe_bits_and_input_quality_stays_independent() {
        let plan = bench_plan();
        let gate = Arc::new(FieldGate::new());
        gate.set_mode(OperatingMode::Run);
        let (mut driver, state) = mock_driver(&plan, gate);
        state.lock().expect("mock").fail_read[0] = true;
        let mut update = InputUpdate::zeros(2);
        driver.poll_inputs(&mut update).unwrap();
        let mut slot_state = vec![
            OutputModuleState {
                quality: Quality::Bad,
                on_bad_quality: BadQualityPolicy::ForceSafe,
            };
            2
        ];
        assert!(driver.fill_output_module_state(&mut slot_state));
        assert_eq!(slot_state[0].quality, Quality::Good);
        assert_eq!(slot_state[0].on_bad_quality, BadQualityPolicy::HoldLast);
        assert_eq!(slot_state[1].quality, Quality::Good);
        driver.stop();
        let state_ref = state.lock().expect("mock");
        assert_eq!(state_ref.releases, vec![(0, 0), (1, 0b10)]);
        drop(state_ref);
        let err = driver.poll_inputs(&mut update).unwrap_err();
        assert!(err.to_string().contains("gpio"));
    }

    #[test]
    fn missing_chip_fails_at_start() {
        let plan = plan_from(
            r#"
  - id: di
    driver: gpio
    config: { chip: gpiochip99, lines: [0] }
    bindings:
      - tag: Pull
        image: I
        bit: 0
"#,
        );
        let mut driver = GpioDriver::new(plan, Arc::new(FieldGate::new()));
        let err = match driver.start() {
            Ok(()) => panic!("gpiochip99 should not open"),
            Err(err) => err,
        };
        let text = err.to_string();
        assert!(text.contains("gpiochip99"), "{text}");
        assert!(!text.contains("PR-17"), "{text}");
    }
}
