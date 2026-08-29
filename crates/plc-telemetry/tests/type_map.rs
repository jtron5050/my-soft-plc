//! PLC → Sparkplug DataType table.

use plc_io::PlcValue;
use plc_telemetry::{
    value_to_sparkplug, MetricType, MetricValue, SP_BOOLEAN, SP_FLOAT, SP_INT16, SP_INT32,
};

#[test]
fn table() {
    assert_eq!(MetricType::Bool.sparkplug_datatype(), SP_BOOLEAN);
    assert_eq!(MetricType::Int.sparkplug_datatype(), SP_INT16);
    assert_eq!(MetricType::Dint.sparkplug_datatype(), SP_INT32);
    assert_eq!(MetricType::Time.sparkplug_datatype(), SP_INT32);
    assert_eq!(MetricType::Real.sparkplug_datatype(), SP_FLOAT);
    let (dt, v) = value_to_sparkplug(PlcValue::Int(-1));
    assert_eq!(dt, SP_INT16);
    assert_eq!(v, MetricValue::Int((-1_i32) as u32));
}
