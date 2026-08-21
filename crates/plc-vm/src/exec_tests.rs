//! Unit tests compiled into the library test target.

#[cfg(test)]
mod tests {
    use crate::{ExecResult, Vm, VmConfig, VmValue};

    #[test]
    fn jmp_if_and_add() {
        let src = r"
.header data_size=8 input_slots=0 output_slots=0
.entry task.main
PUSHI_BOOL 1
JMP_IF do_add
PUSHI_DINT 0
JMP done
do_add:
PUSHI_DINT 2
PUSHI_DINT 3
ADD
done:
ST_DATA 0
HALT
";
        let mut vm = Vm::from_spasm(src, &VmConfig::default()).unwrap();
        assert_eq!(vm.run_entry("task.main", 0).unwrap(), ExecResult::Halted);
        assert_eq!(vm.data().load(0, 0).unwrap(), VmValue::Dint(5));
    }

    #[test]
    fn stack_underflow_errors() {
        let src = r"
.header data_size=0 input_slots=0 output_slots=0
.entry task.main
AND
HALT
";
        let mut vm = Vm::from_spasm(
            src,
            &VmConfig {
                verify: false,
                ..VmConfig::default()
            },
        )
        .unwrap();
        let err = vm.run_entry("task.main", 0).unwrap_err();
        assert!(err.to_string().contains("underflow"));
    }

    #[test]
    fn st_q_marks_written_and_run_at_clears_unwritten() {
        let src = r"
.header data_size=0 input_slots=0 output_slots=2
.entry task.main
PUSHI_BOOL 1
ST_Q 0
HALT
";
        let mut vm = Vm::from_spasm(src, &VmConfig::default()).unwrap();
        assert_eq!(vm.run_entry("task.main", 0).unwrap(), ExecResult::Halted);
        assert!(vm.outputs().written(0));
        assert!(!vm.outputs().written(1));
        vm.outputs_mut().set(1, VmValue::Bool(true), 0).unwrap();
        assert!(vm.outputs().written(1));
        assert_eq!(vm.run_entry("task.main", 0).unwrap(), ExecResult::Halted);
        assert!(vm.outputs().written(0));
        assert!(!vm.outputs().written(1));
    }
}
