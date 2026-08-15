//! I/O map schema: modules, bindings, scale/offset/clamp.

use serde::{Deserialize, Serialize};

/// Root io-map document (architecture illustrative YAML).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IoMap {
    /// Schema version.
    pub version: u32,
    /// Modules (drivers + bindings).
    pub modules: Vec<IoModule>,
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
