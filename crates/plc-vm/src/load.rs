//! Arm-time VM configuration and load helpers.

/// Load / arm options (all allocations happen here, not in the run loop).
#[derive(Debug, Clone)]
pub struct VmConfig {
    /// Primitive instance pool size per kind (covers `CALL_FB instance=` indices).
    pub primitive_instances: usize,
    /// Max instructions per `run_entry` invocation (safety budget).
    pub instruction_budget: u64,
    /// Verify module with `plc_ir::verify_module` before load.
    pub verify: bool,
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            // Enough for fixture bases such as `instance=0x40`.
            primitive_instances: 128,
            instruction_budget: 1_000_000,
            verify: true,
        }
    }
}
