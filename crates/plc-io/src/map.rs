//! I/O map schema: modules, bindings, scale/offset/clamp.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::IoError;
use crate::image::{ProcessImage, SlotMeta, TypedSlot};
use crate::value::PlcValue;

/// Root io-map document (architecture illustrative YAML).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IoMap {
    /// Schema version.
    pub version: u32,
    /// Modules (drivers + bindings).
    pub modules: Vec<IoModule>,
}

impl IoMap {
    /// Parse a YAML io-map from `text`.
    pub fn from_yaml_str(text: &str) -> Result<Self, IoError> {
        let map: Self =
            serde_yaml::from_str(text).map_err(|e| IoError::Map(format!("yaml parse: {e}")))?;
        if map.version != 1 {
            return Err(IoError::Map(format!(
                "unsupported io-map version {} (expected 1)",
                map.version
            )));
        }
        if map.modules.is_empty() {
            return Err(IoError::Map(
                "io-map must contain at least one module".into(),
            ));
        }
        Ok(map)
    }

    /// Load YAML from `path`.
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, IoError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .map_err(|e| IoError::Map(format!("read {}: {e}", path.display())))?;
        Self::from_yaml_str(&text)
    }

    /// Allocate a process image from bindings (slot order = appearance when `slot` omitted).
    ///
    /// PR-14: a single `sim` module is 1:1 with image slots. Later drivers reuse
    /// the same layout builder.
    pub fn build_image(&self) -> Result<ProcessImage, IoError> {
        Ok(self.resolve()?.image)
    }

    /// Same layout as [`Self::build_image`], plus the slot assigned to each binding.
    pub fn resolve(&self) -> Result<ResolvedIoMap, IoError> {
        let mut inputs: Vec<Option<SlotBuild>> = Vec::new();
        let mut outputs: Vec<Option<SlotBuild>> = Vec::new();
        let mut memory: Vec<Option<SlotBuild>> = Vec::new();
        let mut next_i = 0u32;
        let mut next_q = 0u32;
        let mut next_m = 0u32;
        let mut names = BTreeSet::new();
        let mut bindings = Vec::new();

        for (module_index, module) in self.modules.iter().enumerate() {
            if module.id.trim().is_empty() {
                return Err(IoError::Map("module.id must be non-empty".into()));
            }
            if module.driver.trim().is_empty() {
                return Err(IoError::Map(format!(
                    "module '{}' driver must be non-empty",
                    module.id
                )));
            }
            for (binding_index, b) in module.bindings.iter().enumerate() {
                if b.tag.trim().is_empty() {
                    return Err(IoError::Map(format!(
                        "module '{}': binding tag must be non-empty",
                        module.id
                    )));
                }
                if !names.insert(b.tag.clone()) {
                    return Err(IoError::Map(format!("duplicate tag '{}'", b.tag)));
                }
                let (plane, next) = match b.image {
                    ImagePlane::I => (&mut inputs, &mut next_i),
                    ImagePlane::Q => (&mut outputs, &mut next_q),
                    ImagePlane::M => (&mut memory, &mut next_m),
                };
                let slot = match b.slot {
                    Some(s) => s,
                    None => {
                        let s = *next;
                        *next = next.saturating_add(1);
                        s
                    }
                };
                let idx = slot as usize;
                if idx >= plane.len() {
                    plane.resize_with(idx + 1, || None);
                }
                if plane[idx].is_some() {
                    return Err(IoError::Map(format!(
                        "duplicate {} slot {slot} (tag '{}')",
                        plane_name(b.image),
                        b.tag
                    )));
                }
                if b.slot.is_some() {
                    *next = (*next).max(slot.saturating_add(1));
                }
                let safe = match b.image {
                    ImagePlane::Q => Some(parse_safe_state(b.value_type, b.safe_state.as_ref())?),
                    _ => None,
                };
                plane[idx] = Some(SlotBuild {
                    tag: b.tag.clone(),
                    ty: b.value_type,
                    unit: b.unit.clone(),
                    safe,
                });
                bindings.push(ResolvedBinding {
                    module_index,
                    module_id: module.id.clone(),
                    driver: module.driver.clone(),
                    binding_index,
                    slot,
                    image: b.image,
                });
            }
        }

        Ok(ResolvedIoMap {
            image: ProcessImage {
                inputs: fill_slots(&inputs),
                outputs: fill_slots(&outputs),
                memory: fill_slots(&memory),
                input_meta: fill_meta(&inputs, "I"),
                output_meta: fill_meta(&outputs, "Q"),
                memory_meta: fill_meta(&memory, "M"),
                output_safe: outputs
                    .iter()
                    .map(|s| {
                        s.as_ref()
                            .and_then(|b| b.safe)
                            .unwrap_or(PlcValue::Bool(false))
                    })
                    .collect(),
            },
            bindings,
        })
    }
}

/// One binding after slot assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedBinding {
    /// Index into [`IoMap::modules`].
    pub module_index: usize,
    /// Module id copied from the map.
    pub module_id: String,
    /// Driver kind (`sim`, `modbus_tcp`, …).
    pub driver: String,
    /// Index into that module's `bindings`.
    pub binding_index: usize,
    /// Assigned process-image slot.
    pub slot: u32,
    /// Plane this slot belongs to.
    pub image: ImagePlane,
}

/// Process image plus the slot chosen for every binding.
#[derive(Debug, Clone)]
pub struct ResolvedIoMap {
    /// Allocated `%I` / `%Q` / `%M` image.
    pub image: ProcessImage,
    /// Bindings in map order.
    pub bindings: Vec<ResolvedBinding>,
}

#[derive(Clone)]
struct SlotBuild {
    tag: String,
    ty: ValueType,
    unit: String,
    safe: Option<PlcValue>,
}

fn plane_name(p: ImagePlane) -> &'static str {
    match p {
        ImagePlane::I => "%I",
        ImagePlane::Q => "%Q",
        ImagePlane::M => "%M",
    }
}

fn fill_slots(plane: &[Option<SlotBuild>]) -> Vec<TypedSlot> {
    plane
        .iter()
        .map(|s| TypedSlot::zero(s.as_ref().map_or(ValueType::Bool, |b| b.ty)))
        .collect()
}

fn fill_meta(plane: &[Option<SlotBuild>], prefix: &str) -> Vec<SlotMeta> {
    plane
        .iter()
        .enumerate()
        .map(|(i, s)| match s {
            Some(b) => SlotMeta {
                tag: b.tag.clone(),
                ty: b.ty,
                unit: b.unit.clone(),
            },
            None => SlotMeta {
                tag: format!("{prefix}{i}"),
                ty: ValueType::Bool,
                unit: String::new(),
            },
        })
        .collect()
}

fn parse_safe_state(ty: ValueType, raw: Option<&serde_json::Value>) -> Result<PlcValue, IoError> {
    let Some(raw) = raw else {
        return Ok(PlcValue::default_of(ty));
    };
    match ty {
        ValueType::Bool => match raw {
            serde_json::Value::Bool(b) => Ok(PlcValue::Bool(*b)),
            other => Err(IoError::Map(format!("safe_state {other} is not a BOOL"))),
        },
        ValueType::Int => json_i64(raw).map(|n| PlcValue::Int(n as i16)),
        ValueType::Dint => json_i64(raw).map(|n| PlcValue::Dint(n as i32)),
        ValueType::Time => json_i64(raw).map(|n| PlcValue::Time(n as i32)),
        ValueType::Real => match raw {
            serde_json::Value::Number(n) => n
                .as_f64()
                .map(|f| PlcValue::Real(f as f32))
                .ok_or_else(|| IoError::Map("safe_state REAL overflow".into())),
            _ => Err(IoError::Map("safe_state is not a REAL".into())),
        },
    }
}

fn json_i64(raw: &serde_json::Value) -> Result<i64, IoError> {
    match raw {
        serde_json::Value::Number(n) => n
            .as_i64()
            .ok_or_else(|| IoError::Map("safe_state integer overflow".into())),
        serde_json::Value::Bool(b) => Ok(i64::from(*b)),
        _ => Err(IoError::Map("safe_state is not an integer".into())),
    }
}

/// One I/O module backed by a driver instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IoModule {
    /// Module id (diagnostics / quality system tags).
    pub id: String,
    /// Driver kind: `sim`, `gpio`, `modbus_tcp`.
    pub driver: String,
    /// Opaque driver config (chip, endpoint, …) as YAML mapping values.
    #[serde(default)]
    pub config: serde_yaml::Value,
    /// Policy when module quality is Bad (outputs).
    #[serde(default)]
    pub on_bad_quality: BadQualityPolicy,
    /// Tag bindings into the process image.
    #[serde(default)]
    pub bindings: Vec<IoBinding>,
}

/// Output behavior when module quality is Bad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BadQualityPolicy {
    /// Force outputs to `safe_state` (architecture default).
    #[default]
    ForceSafe,
    /// Hold last good program / field value.
    HoldLast,
}

/// Single tag ↔ image binding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IoBinding {
    /// Logical tag name.
    pub tag: String,
    /// Process image plane: `I` or `Q` (and optionally `M` for tests).
    pub image: ImagePlane,
    /// Value type (default BOOL).
    #[serde(default, rename = "type")]
    pub value_type: ValueType,
    /// Bit index within a digital pack (optional).
    #[serde(default)]
    pub bit: Option<u32>,
    /// Slot index in the typed image (assigned by mapper if omitted in early maps).
    #[serde(default)]
    pub slot: Option<u32>,
    /// Engineering scale: `eng = raw * scale + offset`.
    #[serde(default = "default_scale")]
    pub scale: f64,
    /// Engineering offset.
    #[serde(default)]
    pub offset: f64,
    /// Optional `[min, max]` clamp in engineering units.
    #[serde(default)]
    pub clamp: Option<[f64; 2]>,
    /// Engineering unit label.
    #[serde(default)]
    pub unit: String,
    /// Safe state for outputs (BOOL false / numeric 0 if omitted).
    #[serde(default)]
    pub safe_state: Option<serde_json::Value>,
    /// Fieldbus register address (Modbus, etc.).
    #[serde(default)]
    pub register: Option<u32>,
    /// Register class.
    #[serde(default)]
    pub register_type: Option<RegisterType>,
    /// Raw field type before scale.
    #[serde(default)]
    pub raw_type: Option<RawType>,
}

fn default_scale() -> f64 {
    1.0
}

/// Image plane selector in bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ImagePlane {
    /// Inputs `%I`.
    I,
    /// Outputs `%Q`.
    Q,
    /// Memory `%M` (rare in io-map; allowed for sim).
    M,
}

impl ImagePlane {
    /// Direction implied by plane for drivers.
    #[must_use]
    pub const fn direction(self) -> BindingDirection {
        match self {
            Self::I => BindingDirection::Input,
            Self::Q => BindingDirection::Output,
            Self::M => BindingDirection::Memory,
        }
    }
}

/// Binding direction relative to the field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingDirection {
    /// Field → `%I`.
    Input,
    /// `%Q` → field.
    Output,
    /// Memory (not field-backed).
    Memory,
}

/// Logical PLC type of a binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum ValueType {
    /// BOOL.
    #[default]
    Bool,
    /// INT (i16).
    Int,
    /// DINT (i32).
    Dint,
    /// REAL (f32).
    Real,
    /// TIME (i32 ms).
    Time,
}

/// Modbus / field register class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegisterType {
    /// Holding register.
    Holding,
    /// Input register.
    Input,
    /// Coil.
    Coil,
    /// Discrete input.
    Discrete,
}

/// Raw field encoding before scale/offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum RawType {
    /// BOOL.
    Bool,
    /// INT.
    Int,
    /// DINT.
    Dint,
    /// UINT (stored as i32 domain after cast).
    Uint,
    /// REAL.
    Real,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assigns_slots_in_binding_order() {
        let yaml = r#"
version: 1
modules:
  - id: sim_line
    driver: sim
    bindings:
      - tag: Conveyor1/StartCmd
        image: I
      - tag: Conveyor1/RunFwd
        image: Q
        safe_state: false
      - tag: Conveyor1/Fault
        image: Q
"#;
        let map = IoMap::from_yaml_str(yaml).unwrap();
        let image = map.build_image().unwrap();
        assert_eq!(image.inputs.len(), 1);
        assert_eq!(image.outputs.len(), 2);
        assert_eq!(image.input_meta[0].tag, "Conveyor1/StartCmd");
        assert_eq!(image.output_meta[0].tag, "Conveyor1/RunFwd");
        assert_eq!(image.output_meta[1].tag, "Conveyor1/Fault");
        assert_eq!(image.output_safe[0], PlcValue::Bool(false));
    }

    #[test]
    fn rejects_duplicate_tags() {
        let yaml = r#"
version: 1
modules:
  - id: a
    driver: sim
    bindings:
      - tag: X
        image: I
      - tag: X
        image: Q
"#;
        let map = IoMap::from_yaml_str(yaml).unwrap();
        let err = map.build_image().unwrap_err();
        assert!(err.to_string().contains("duplicate tag"));
    }

    #[test]
    fn loads_sim_plant_io_map() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../samples/configs/sim-plant-io-map.yaml");
        let map = IoMap::load_from_path(&path).expect("sim-plant-io-map.yaml");
        let image = map.build_image().unwrap();
        assert_eq!(image.inputs.len(), 6);
        assert_eq!(image.outputs.len(), 3);
        assert_eq!(image.input_meta[0].tag, "Conveyor1/StartCmd");
        assert_eq!(image.input_meta[5].tag, "Conveyor1/LocalMode");
        assert_eq!(image.output_meta[0].tag, "Conveyor1/RunFwd");
        assert_eq!(image.output_meta[2].tag, "Conveyor1/Ready");
    }

    #[test]
    fn resolves_modbus_rack_golden() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../samples/configs/modbus-rack-io-map.yaml");
        let map = IoMap::load_from_path(&path).expect("modbus-rack-io-map.yaml");
        let resolved = map.resolve().unwrap();
        assert_eq!(resolved.image.inputs.len(), 1);
        assert_eq!(resolved.image.outputs.len(), 1);
        assert_eq!(resolved.image.input_meta[0].tag, "Silo1.Level_eu");
        assert_eq!(resolved.image.input_meta[0].ty, ValueType::Real);
        assert_eq!(resolved.image.input_meta[0].unit, "pct");
        assert_eq!(resolved.image.output_meta[0].tag, "Silo1.DumpGate");
        assert_eq!(resolved.image.output_safe[0], PlcValue::Bool(false));
        assert_eq!(map.modules[0].driver, "modbus_tcp");
        assert_eq!(map.modules[0].on_bad_quality, BadQualityPolicy::ForceSafe);
        let level = &map.modules[0].bindings[0];
        assert!((level.scale - 0.1).abs() < 1e-12);
        assert!(level.offset.abs() < 1e-12);
        let [lo, hi] = level.clamp.expect("clamp");
        assert!(lo.abs() < 1e-12);
        assert!((hi - 100.0).abs() < 1e-12);
        assert_eq!(level.register, Some(40_001));
        assert_eq!(level.register_type, Some(RegisterType::Holding));
        assert_eq!(level.raw_type, Some(RawType::Int));
        assert_eq!(resolved.bindings.len(), 2);
        assert_eq!(resolved.bindings[0].slot, 0);
        assert_eq!(resolved.bindings[0].image, ImagePlane::I);
        assert_eq!(resolved.bindings[1].slot, 0);
        assert_eq!(resolved.bindings[1].image, ImagePlane::Q);
        assert_eq!(resolved.bindings[0].module_id, "remote_rack_a");
    }
}
