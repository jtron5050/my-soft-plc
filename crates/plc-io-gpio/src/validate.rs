//! Typed checks for `driver: gpio` modules.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use plc_io::{BadQualityPolicy, ImagePlane, IoBinding, IoError, IoMap, ResolvedIoMap, ValueType};
use serde::Deserialize;

/// Validated GPIO modules from an io-map. Other drivers are skipped.
#[derive(Debug, Clone, PartialEq)]
pub struct GpioPlan {
    /// Modules in map order.
    pub modules: Vec<PlannedModule>,
}

/// One gpiochip request.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedModule {
    /// Module id.
    pub id: String,
    /// `gpiochipN` name.
    pub chip: String,
    /// Absolute character-device path.
    pub path: PathBuf,
    /// Input or output. A module is never mixed.
    pub direction: LineDirection,
    /// Line offsets, request order. `bit` indexes this vector.
    pub offsets: Vec<u32>,
    /// Kernel active-low for every line in the module.
    pub active_low: bool,
    /// Pin bias requested at claim time.
    pub bias: Bias,
    /// Output drive. `None` on inputs.
    pub drive: Option<Drive>,
    /// Output policy while this module is Bad.
    pub on_bad: BadQualityPolicy,
    /// Bindings in map order.
    pub points: Vec<PlannedPoint>,
}

/// One BOOL line binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedPoint {
    /// Tag name.
    pub tag: String,
    /// Process-image slot.
    pub slot: usize,
    /// Index into [`PlannedModule::offsets`].
    pub bit: u8,
    /// gpiochip line offset (`offsets[bit]`).
    pub offset: u32,
    /// Output `safe_state`. False for inputs.
    pub safe: bool,
}

/// Direction of every binding in a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineDirection {
    /// `%I` lines.
    Input,
    /// `%Q` lines.
    Output,
}

/// Optional pin bias. Default leaves the controller setting alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Bias {
    /// Do not set a bias flag.
    #[default]
    AsIs,
    /// Enable the pull-up.
    PullUp,
    /// Enable the pull-down.
    PullDown,
    /// Disable bias.
    Disabled,
}

/// Output drive. Inputs reject this field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Drive {
    /// Open-drain. Default for outputs.
    OpenDrain,
    /// Push-pull.
    PushPull,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModuleConfig {
    chip: String,
    lines: Vec<u32>,
    #[serde(default)]
    active_low: bool,
    #[serde(default)]
    bias: Bias,
    #[serde(default)]
    drive: Option<Drive>,
}

/// Validate every `gpio` module. Other drivers are ignored.
///
/// # Errors
/// Returns [`IoError::Map`] when a GPIO module is incomplete or inconsistent.
pub fn validate_map(map: &IoMap, resolved: &ResolvedIoMap) -> Result<GpioPlan, IoError> {
    let mut modules = Vec::new();
    let mut ids = BTreeSet::new();
    let mut claimed: BTreeMap<(String, u32), String> = BTreeMap::new();
    for (module_index, module) in map.modules.iter().enumerate() {
        if module.driver != "gpio" {
            continue;
        }
        if !ids.insert(module.id.clone()) {
            return Err(IoError::Map(format!(
                "duplicate gpio module id '{}'",
                module.id
            )));
        }
        let planned = validate_module(module_index, module, resolved)?;
        for offset in &planned.offsets {
            if let Some(prev) = claimed.insert((planned.chip.clone(), *offset), planned.id.clone())
            {
                return Err(IoError::Map(format!(
                    "{} line {offset} is claimed by module '{prev}' and '{}'",
                    planned.chip, planned.id
                )));
            }
        }
        modules.push(planned);
    }
    Ok(GpioPlan { modules })
}

fn validate_module(
    module_index: usize,
    module: &plc_io::IoModule,
    resolved: &ResolvedIoMap,
) -> Result<PlannedModule, IoError> {
    let cfg: ModuleConfig = serde_yaml::from_value(module.config.clone())
        .map_err(|err| IoError::Map(format!("module '{}': gpio config: {err}", module.id)))?;
    let chip = normalize_chip(&cfg.chip)
        .map_err(|err| IoError::Map(format!("module '{}': {err}", module.id)))?;
    if cfg.lines.is_empty() {
        return Err(IoError::Map(format!(
            "module '{}': lines must be non-empty",
            module.id
        )));
    }
    if cfg.lines.len() > 64 {
        return Err(IoError::Map(format!(
            "module '{}': lines exceed 64",
            module.id
        )));
    }
    let mut seen_offsets = BTreeSet::new();
    for offset in &cfg.lines {
        if !seen_offsets.insert(*offset) {
            return Err(IoError::Map(format!(
                "module '{}': duplicate line offset {offset}",
                module.id
            )));
        }
    }
    if module.bindings.is_empty() {
        return Err(IoError::Map(format!(
            "module '{}': gpio module needs at least one binding",
            module.id
        )));
    }

    let mut direction: Option<LineDirection> = None;
    let mut used = vec![false; cfg.lines.len()];
    let mut next = 0usize;
    let mut points = Vec::new();
    for (binding_index, binding) in module.bindings.iter().enumerate() {
        let point_dir = check_binding(binding)?;
        match direction {
            None => direction = Some(point_dir),
            Some(existing) if existing != point_dir => {
                return Err(IoError::Map(format!(
                    "module '{}': gpio module must be all inputs or all outputs",
                    module.id
                )));
            }
            Some(_) => {}
        }
        let bit = match binding.bit {
            Some(bit) => bit,
            None => {
                while next < used.len() && used[next] {
                    next += 1;
                }
                let bit = u32::try_from(next).unwrap_or(u32::MAX);
                next += 1;
                bit
            }
        };
        if bit as usize >= cfg.lines.len() {
            return Err(IoError::Map(format!(
                "binding '{}': bit {bit} is outside lines",
                binding.tag
            )));
        }
        let bit_index = bit as usize;
        if used[bit_index] {
            return Err(IoError::Map(format!(
                "binding '{}': duplicate bit {bit}",
                binding.tag
            )));
        }
        used[bit_index] = true;
        let slot = resolved
            .bindings
            .iter()
            .find(|item| item.module_index == module_index && item.binding_index == binding_index)
            .map(|item| item.slot as usize)
            .ok_or_else(|| {
                IoError::Map(format!("binding '{}': missing resolved slot", binding.tag))
            })?;
        let safe = output_safe(resolved, point_dir, slot, &binding.tag)?;
        let bit_u8 = u8::try_from(bit).map_err(|_| {
            IoError::Map(format!(
                "binding '{}': bit {bit} is outside lines",
                binding.tag
            ))
        })?;
        points.push(PlannedPoint {
            tag: binding.tag.clone(),
            slot,
            bit: bit_u8,
            offset: cfg.lines[bit_index],
            safe,
        });
    }
    let direction = direction.ok_or_else(|| {
        IoError::Map(format!(
            "module '{}': gpio module needs at least one binding",
            module.id
        ))
    })?;
    if direction == LineDirection::Input && cfg.drive.is_some() {
        return Err(IoError::Map(format!(
            "module '{}': drive is only valid on outputs",
            module.id
        )));
    }
    let drive = match direction {
        LineDirection::Input => None,
        LineDirection::Output => Some(cfg.drive.unwrap_or(Drive::OpenDrain)),
    };
    Ok(PlannedModule {
        id: module.id.clone(),
        path: PathBuf::from(format!("/dev/{chip}")),
        chip,
        direction,
        offsets: cfg.lines,
        active_low: cfg.active_low,
        bias: cfg.bias,
        drive,
        on_bad: module.on_bad_quality,
        points,
    })
}

fn check_binding(binding: &IoBinding) -> Result<LineDirection, IoError> {
    if binding.image == ImagePlane::M {
        return Err(IoError::Map(format!(
            "binding '{}': gpio does not back %M",
            binding.tag
        )));
    }
    if binding.value_type != ValueType::Bool {
        return Err(IoError::Map(format!(
            "binding '{}': gpio lines are BOOL",
            binding.tag
        )));
    }
    if binding.register.is_some() || binding.register_type.is_some() || binding.raw_type.is_some() {
        return Err(IoError::Map(format!(
            "binding '{}': gpio does not use registers",
            binding.tag
        )));
    }
    // Schema defaults are exact 1.0 and 0.0. A tolerance would accept a non-default scale.
    #[allow(clippy::float_cmp)]
    if binding.scale != 1.0 || binding.offset != 0.0 || binding.clamp.is_some() {
        return Err(IoError::Map(format!(
            "binding '{}': gpio does not scale",
            binding.tag
        )));
    }
    Ok(if binding.image == ImagePlane::I {
        LineDirection::Input
    } else {
        LineDirection::Output
    })
}

fn output_safe(
    resolved: &ResolvedIoMap,
    direction: LineDirection,
    slot: usize,
    tag: &str,
) -> Result<bool, IoError> {
    if direction != LineDirection::Output {
        return Ok(false);
    }
    match resolved.image.output_safe.get(slot) {
        Some(plc_io::PlcValue::Bool(value)) => Ok(*value),
        Some(_) => Err(IoError::Map(format!(
            "binding '{tag}': safe_state is not BOOL"
        ))),
        None => Ok(false),
    }
}

fn normalize_chip(raw: &str) -> Result<String, String> {
    let name = raw.strip_prefix("/dev/").unwrap_or(raw);
    let Some(num) = name.strip_prefix("gpiochip") else {
        return Err(format!("chip '{raw}' must be gpiochipN or /dev/gpiochipN"));
    };
    if num.is_empty() || !num.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("chip '{raw}' must be gpiochipN or /dev/gpiochipN"));
    }
    Ok(name.to_string())
}
