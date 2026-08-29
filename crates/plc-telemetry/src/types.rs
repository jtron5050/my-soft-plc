//! PLC → Sparkplug DataType and OPC-DA quality mapping.

use plc_io::ValueType;
use plc_types::Quality;

use crate::protobuf::MetricValue;

/// Sparkplug DataType: Int16.
pub const SP_INT16: u32 = 2;
/// Sparkplug DataType: Int32.
pub const SP_INT32: u32 = 3;
/// Sparkplug DataType: Int64.
pub const SP_INT64: u32 = 4;
/// Sparkplug DataType: UInt64.
pub const SP_UINT64: u32 = 8;
/// Sparkplug DataType: Float.
pub const SP_FLOAT: u32 = 9;
/// Sparkplug DataType: Boolean.
pub const SP_BOOLEAN: u32 = 11;
/// Sparkplug DataType: String.
pub const SP_STRING: u32 = 12;

/// OPC DA / Sparkplug `Quality` property: Good.
pub const QUALITY_GOOD: i32 = 192;
/// OPC DA / Sparkplug `Quality` property: Uncertain.
pub const QUALITY_UNCERTAIN: i32 = 64;
/// OPC DA / Sparkplug `Quality` property: Bad.
pub const QUALITY_BAD: i32 = 0;

/// Metric type used in the tag catalog (includes String / LINT).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricType {
    /// BOOL.
    Bool,
    /// INT (i16).
    Int,
    /// DINT (i32).
    Dint,
    /// REAL (f32).
    Real,
    /// TIME (i32 ms).
    Time,
    /// LINT (i64) — catalog-only; process image has no LINT today.
    Lint,
    /// STRING — node metrics such as `SYSTEM/Mode`.
    String,
}

impl From<ValueType> for MetricType {
    fn from(value: ValueType) -> Self {
        match value {
            ValueType::Bool => Self::Bool,
            ValueType::Int => Self::Int,
            ValueType::Dint => Self::Dint,
            ValueType::Real => Self::Real,
            ValueType::Time => Self::Time,
        }
    }
}

impl MetricType {
    /// Sparkplug B `datatype` enumeration value.
    #[must_use]
    pub const fn sparkplug_datatype(self) -> u32 {
        match self {
            Self::Bool => SP_BOOLEAN,
            Self::Int => SP_INT16,
            Self::Dint | Self::Time => SP_INT32,
            Self::Real => SP_FLOAT,
            Self::Lint => SP_INT64,
            Self::String => SP_STRING,
        }
    }

    /// Default birth value when no live sample is available.
    #[must_use]
    pub fn default_value(self) -> MetricValue {
        match self {
            Self::Bool => MetricValue::Bool(false),
            Self::Int | Self::Dint | Self::Time => MetricValue::Int(0),
            Self::Real => MetricValue::Float(0.0),
            Self::Lint => MetricValue::Long(0),
            Self::String => MetricValue::String(String::new()),
        }
    }
}

/// Map a process-image value onto Sparkplug datatype + payload.
#[must_use]
pub fn value_to_sparkplug(value: plc_io::PlcValue) -> (u32, MetricValue) {
    match value {
        plc_io::PlcValue::Bool(b) => (SP_BOOLEAN, MetricValue::Bool(b)),
        plc_io::PlcValue::Int(v) => (SP_INT16, MetricValue::Int(i32::from(v) as u32)),
        plc_io::PlcValue::Dint(v) | plc_io::PlcValue::Time(v) => {
            (SP_INT32, MetricValue::Int(v as u32))
        }
        plc_io::PlcValue::Real(v) => (SP_FLOAT, MetricValue::Float(v)),
    }
}

/// OPC DA quality code stored in the Sparkplug `Quality` Int32 property.
#[must_use]
pub const fn quality_code(quality: Quality) -> i32 {
    match quality {
        Quality::Good => QUALITY_GOOD,
        Quality::Uncertain => QUALITY_UNCERTAIN,
        Quality::Bad => QUALITY_BAD,
    }
}

/// Combine tag quality with wall-clock sync (KD-19). Unsynced clocks never
/// publish Good; Bad is worse than Uncertain.
#[must_use]
pub fn publish_quality(tag: Quality, clock_synchronized: bool) -> Quality {
    if matches!(tag, Quality::Bad) {
        return Quality::Bad;
    }
    if !clock_synchronized {
        return Quality::Uncertain;
    }
    tag
}

#[cfg(test)]
mod tests {
    use super::*;
    use plc_io::PlcValue;

    #[test]
    fn plc_type_map() {
        assert_eq!(
            value_to_sparkplug(PlcValue::Bool(true)),
            (SP_BOOLEAN, MetricValue::Bool(true))
        );
        assert_eq!(
            value_to_sparkplug(PlcValue::Int(-2)),
            (SP_INT16, MetricValue::Int((-2_i32) as u32))
        );
        assert_eq!(
            value_to_sparkplug(PlcValue::Dint(7)),
            (SP_INT32, MetricValue::Int(7))
        );
        assert_eq!(
            value_to_sparkplug(PlcValue::Time(1000)),
            (SP_INT32, MetricValue::Int(1000))
        );
        let (dt, v) = value_to_sparkplug(PlcValue::Real(1.5));
        assert_eq!(dt, SP_FLOAT);
        assert_eq!(v, MetricValue::Float(1.5));
    }

    #[test]
    fn quality_codes() {
        assert_eq!(quality_code(Quality::Good), 192);
        assert_eq!(quality_code(Quality::Uncertain), 64);
        assert_eq!(quality_code(Quality::Bad), 0);
    }

    #[test]
    fn unsynced_never_good() {
        assert_eq!(publish_quality(Quality::Good, false), Quality::Uncertain);
        assert_eq!(publish_quality(Quality::Bad, false), Quality::Bad);
        assert_eq!(publish_quality(Quality::Good, true), Quality::Good);
    }
}
