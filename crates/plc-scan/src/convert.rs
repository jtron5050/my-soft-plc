//! Process-image [`PlcValue`] ↔ VM [`VmValue`].

use plc_io::PlcValue;
use plc_vm::VmValue;

/// Map an I/O image value into a VM slot.
#[must_use]
pub fn plc_to_vm(value: PlcValue) -> VmValue {
    match value {
        PlcValue::Bool(b) => VmValue::Bool(b),
        PlcValue::Int(v) => VmValue::Int(v),
        PlcValue::Dint(v) => VmValue::Dint(v),
        PlcValue::Real(v) => VmValue::Real(v),
        PlcValue::Time(v) => VmValue::Time(v),
    }
}

/// Map a VM slot into an I/O image value.
///
/// `LINT` is not an I/O-plane type; it is truncated to `DINT`.
#[must_use]
pub fn vm_to_plc(value: VmValue) -> PlcValue {
    match value {
        VmValue::Bool(b) => PlcValue::Bool(b),
        VmValue::Int(v) => PlcValue::Int(v),
        VmValue::Dint(v) => PlcValue::Dint(v),
        VmValue::Real(v) => PlcValue::Real(v),
        VmValue::Time(v) => PlcValue::Time(v),
        VmValue::Lint(v) => PlcValue::Dint(v as i32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_bool() {
        assert_eq!(
            vm_to_plc(plc_to_vm(PlcValue::Bool(true))),
            PlcValue::Bool(true)
        );
    }

    #[test]
    fn lint_truncates_to_dint() {
        assert_eq!(vm_to_plc(VmValue::Lint(5)), PlcValue::Dint(5));
    }
}
