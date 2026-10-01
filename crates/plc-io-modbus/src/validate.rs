//! Typed checks for `driver: modbus_tcp` modules.

use std::time::Duration;

use plc_io::{
    BadQualityPolicy, BindingDirection, ImagePlane, IoError, IoMap, PlcValue, RawType,
    RegisterType, ResolvedIoMap, ValueType,
};
use serde::Deserialize;

use crate::address::{self, is_bit_table};

/// Validated Modbus modules from an io-map. Sim and other drivers are skipped.
#[derive(Debug, Clone, PartialEq)]
pub struct ModbusPlan {
    /// Modules in map order.
    pub modules: Vec<PlannedModule>,
}

/// One Modbus TCP connection and its points.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedModule {
    /// Module id.
    pub id: String,
    /// Host or address (not yet resolved).
    pub host: String,
    /// TCP port.
    pub port: u16,
    /// Modbus unit id (`1..=255`).
    pub unit: u8,
    /// Poll period.
    pub poll_ms: u64,
    /// Age after which a quiet module is Bad.
    pub stale: Duration,
    /// Socket connect and I/O timeout.
    pub timeout: Duration,
    /// Output policy while this module is Bad.
    pub on_bad: BadQualityPolicy,
    /// Points in map order.
    pub points: Vec<PlannedPoint>,
}

/// One decoded binding.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedPoint {
    /// Tag name.
    pub tag: String,
    /// Process-image slot.
    pub slot: usize,
    /// Field direction.
    pub direction: BindingDirection,
    /// Modbus table.
    pub table: RegisterType,
    /// 0-based PDU address.
    pub pdu: u16,
    /// Coils occupied, or registers occupied.
    pub words: u16,
    /// Raw field encoding.
    pub raw: RawType,
    /// Bit inside a holding or input register.
    pub bit: Option<u8>,
    /// Engineering type stored in the process image.
    pub value_type: ValueType,
    /// `eng = raw * scale + offset`.
    pub scale: f64,
    /// Engineering offset.
    pub offset: f64,
    /// Input clamp. Cleared for outputs.
    pub clamp: Option<[f64; 2]>,
    /// Engineering safe state for outputs.
    pub safe: PlcValue,
    /// Index in this module's input vector.
    pub input_pos: Option<usize>,
    /// Index in this module's output vector.
    pub output_pos: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModuleConfig {
    endpoint: String,
    #[serde(default = "default_unit")]
    unit: u16,
    #[serde(default = "default_poll")]
    poll_ms: u64,
    #[serde(default)]
    stale_ms: Option<u64>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

fn default_unit() -> u16 {
    1
}

fn default_poll() -> u64 {
    100
}

/// Validate every `modbus_tcp` module. Other drivers are ignored.
///
/// # Errors
/// Returns [`IoError::Map`] when a Modbus module is incomplete or inconsistent.
pub fn validate_map(map: &IoMap, resolved: &ResolvedIoMap) -> Result<ModbusPlan, IoError> {
    let mut modules = Vec::new();
    for (module_index, module) in map.modules.iter().enumerate() {
        if module.driver != "modbus_tcp" {
            continue;
        }
        modules.push(validate_module(module_index, module, resolved)?);
    }
    Ok(ModbusPlan { modules })
}

fn validate_module(
    module_index: usize,
    module: &plc_io::IoModule,
    resolved: &ResolvedIoMap,
) -> Result<PlannedModule, IoError> {
    let cfg: ModuleConfig = serde_yaml::from_value(module.config.clone())
        .map_err(|e| IoError::Map(format!("module '{}': modbus config: {e}", module.id)))?;
    if cfg.poll_ms == 0 {
        return Err(IoError::Map(format!(
            "module '{}': poll_ms must be > 0",
            module.id
        )));
    }
    if !(1..=255).contains(&cfg.unit) {
        return Err(IoError::Map(format!(
            "module '{}': unit must be 1..=255",
            module.id
        )));
    }
    let stale_ms = cfg.stale_ms.unwrap_or(cfg.poll_ms.saturating_mul(3));
    if stale_ms < cfg.poll_ms {
        return Err(IoError::Map(format!(
            "module '{}': stale_ms must be >= poll_ms",
            module.id
        )));
    }
    let timeout_ms = cfg.timeout_ms.unwrap_or(cfg.poll_ms);
    if timeout_ms == 0 {
        return Err(IoError::Map(format!(
            "module '{}': timeout_ms must be > 0",
            module.id
        )));
    }
    let (host, port) = parse_endpoint(&cfg.endpoint)
        .map_err(|e| IoError::Map(format!("module '{}': {e}", module.id)))?;

    let mut points = Vec::new();
    let mut input_pos = 0usize;
    let mut output_pos = 0usize;
    let mut spans: Vec<(RegisterType, u32, u32, String)> = Vec::new();

    for (binding_index, binding) in module.bindings.iter().enumerate() {
        if binding.image == ImagePlane::M {
            return Err(IoError::Map(format!(
                "binding '{}': modbus does not back %M",
                binding.tag
            )));
        }
        let Some(register) = binding.register else {
            return Err(IoError::Map(format!(
                "binding '{}': register is required",
                binding.tag
            )));
        };
        let (table, pdu) = address::decode(register, binding.register_type)
            .map_err(|e| IoError::Map(format!("binding '{}': {e}", binding.tag)))?;
        if binding.value_type == ValueType::Time {
            return Err(IoError::Map(format!(
                "binding '{}': TIME is not a modbus raw type",
                binding.tag
            )));
        }
        let raw = match binding.raw_type {
            Some(raw) => raw,
            None => default_raw(binding.value_type)
                .map_err(|e| IoError::Map(format!("binding '{}': {e}", binding.tag)))?,
        };
        let bit = check_bit(table, raw, binding.bit, binding.value_type, &binding.tag)?;
        let direction = binding.image.direction();
        if direction == BindingDirection::Output
            && matches!(table, RegisterType::Input | RegisterType::Discrete)
        {
            return Err(IoError::Map(format!(
                "binding '{}': {} registers are read-only",
                binding.tag,
                address::table_name(table)
            )));
        }
        if direction == BindingDirection::Output && binding.scale == 0.0 {
            return Err(IoError::Map(format!(
                "binding '{}': output scale must be non-zero",
                binding.tag
            )));
        }
        if !binding.scale.is_finite() || !binding.offset.is_finite() {
            return Err(IoError::Map(format!(
                "binding '{}': scale and offset must be finite",
                binding.tag
            )));
        }
        let words = if is_bit_table(table) {
            1
        } else {
            raw_words(raw)
        };
        let slot = resolved
            .bindings
            .iter()
            .find(|b| b.module_index == module_index && b.binding_index == binding_index)
            .map(|b| b.slot as usize)
            .ok_or_else(|| {
                IoError::Map(format!(
                    "binding '{}': missing slot assignment",
                    binding.tag
                ))
            })?;
        let end = u32::from(pdu) + u32::from(words);
        spans.push((table, u32::from(pdu), end, binding.tag.clone()));
        let (in_pos, out_pos) = match direction {
            BindingDirection::Input => {
                let pos = input_pos;
                input_pos += 1;
                (Some(pos), None)
            }
            BindingDirection::Output => {
                let pos = output_pos;
                output_pos += 1;
                (None, Some(pos))
            }
            BindingDirection::Memory => (None, None),
        };
        let safe = if direction == BindingDirection::Output {
            resolved
                .image
                .output_safe
                .get(slot)
                .copied()
                .unwrap_or(PlcValue::Bool(false))
        } else {
            PlcValue::default_of(binding.value_type)
        };
        points.push(PlannedPoint {
            tag: binding.tag.clone(),
            slot,
            direction,
            table,
            pdu,
            words,
            raw,
            bit,
            value_type: binding.value_type,
            scale: binding.scale,
            offset: binding.offset,
            clamp: if direction == BindingDirection::Input {
                binding.clamp
            } else {
                None
            },
            safe,
            input_pos: in_pos,
            output_pos: out_pos,
        });
    }
    check_overlaps(&module.id, &mut spans)?;

    Ok(PlannedModule {
        id: module.id.clone(),
        host,
        port,
        unit: u8::try_from(cfg.unit).unwrap_or(1),
        poll_ms: cfg.poll_ms,
        stale: Duration::from_millis(stale_ms),
        timeout: Duration::from_millis(timeout_ms),
        on_bad: module.on_bad_quality,
        points,
    })
}

fn check_overlaps(
    module_id: &str,
    spans: &mut [(RegisterType, u32, u32, String)],
) -> Result<(), IoError> {
    spans.sort_by_key(|(table, start, _, _)| (*table as u8, *start));
    for pair in spans.windows(2) {
        let (t0, _, e0, tag0) = &pair[0];
        let (t1, s1, _, tag1) = &pair[1];
        if t0 == t1 && *s1 < *e0 {
            return Err(IoError::Map(format!(
                "module '{module_id}': registers overlap ({tag0} and {tag1})"
            )));
        }
    }
    Ok(())
}

fn check_bit(
    table: RegisterType,
    raw: RawType,
    bit: Option<u32>,
    value_type: ValueType,
    tag: &str,
) -> Result<Option<u8>, IoError> {
    if is_bit_table(table) {
        if bit.is_some() {
            return Err(IoError::Map(format!(
                "binding '{tag}': bit is not used on coils or discrete inputs"
            )));
        }
        if raw != RawType::Bool || value_type != ValueType::Bool {
            return Err(IoError::Map(format!(
                "binding '{tag}': coils and discrete inputs are BOOL"
            )));
        }
        return Ok(None);
    }
    if raw == RawType::Bool {
        if value_type != ValueType::Bool {
            return Err(IoError::Map(format!(
                "binding '{tag}': BOOL raw values map to BOOL tags"
            )));
        }
        let Some(bit) = bit else {
            return Err(IoError::Map(format!(
                "binding '{tag}': holding/input BOOL requires bit"
            )));
        };
        if bit > 15 {
            return Err(IoError::Map(format!(
                "binding '{tag}': bit {bit} is outside 0..=15"
            )));
        }
        return Ok(Some(u8::try_from(bit).unwrap_or(0)));
    }
    if bit.is_some() {
        return Err(IoError::Map(format!(
            "binding '{tag}': bit is only valid for BOOL raw values"
        )));
    }
    Ok(None)
}

fn default_raw(ty: ValueType) -> Result<RawType, String> {
    match ty {
        ValueType::Bool => Ok(RawType::Bool),
        ValueType::Int => Ok(RawType::Int),
        ValueType::Dint => Ok(RawType::Dint),
        ValueType::Real => Ok(RawType::Real),
        ValueType::Time => Err("TIME is not a modbus raw type".into()),
    }
}

fn raw_words(raw: RawType) -> u16 {
    match raw {
        RawType::Bool | RawType::Int | RawType::Uint => 1,
        RawType::Dint | RawType::Real => 2,
    }
}

fn parse_endpoint(raw: &str) -> Result<(String, u16), String> {
    let raw = raw.trim();
    let Some((host, port)) = raw.rsplit_once(':') else {
        return Err("endpoint must be host:port".into());
    };
    let host = host.trim();
    let host = if host.starts_with('[') && host.ends_with(']') && host.len() >= 2 {
        &host[1..host.len() - 1]
    } else if host.contains(':') {
        return Err("endpoint must be host:port".into());
    } else {
        host
    };
    if host.is_empty() {
        return Err("endpoint must be host:port".into());
    }
    let port: u16 = port
        .trim()
        .parse()
        .map_err(|_| "endpoint must be host:port".to_string())?;
    if port == 0 {
        return Err("endpoint port must be non-zero".into());
    }
    Ok((host.to_string(), port))
}
