//! Pluggable I/O driver trait (architecture conceptual interface).

use plc_types::Quality;

use crate::error::IoError;
use crate::value::PlcValue;

/// Input poll result filled by [`IoDriver::poll_inputs`].
#[derive(Debug, Clone)]
pub struct InputUpdate {
    /// Values for this driver's image region (mapper maps to global slots).
    pub values: Vec<PlcValue>,
    /// Parallel quality.
    pub quality: Vec<Quality>,
    /// Driver-local sequence number.
    pub seq: u64,
}

impl InputUpdate {
    /// Empty update with `n` slots.
    #[must_use]
    pub fn zeros(n: usize) -> Self {
        Self {
            values: vec![PlcValue::Bool(false); n],
            quality: vec![Quality::Good; n],
            seq: 0,
        }
    }
}

/// Output image view presented to drivers.
#[derive(Debug, Clone)]
pub struct OutputImage {
    /// Effective output values (already force/safe resolved by mapper).
    pub values: Vec<PlcValue>,
    /// When true, driver must drive safe/de-energized states.
    pub force_safe: bool,
}

/// Lightweight driver diagnostics.
#[derive(Debug, Clone, Default)]
pub struct DriverDiag {
    /// Driver-specific status text.
    pub status: String,
    /// Consecutive poll failures.
    pub fail_count: u32,
    /// Last sequence published.
    pub last_seq: u64,
}

/// Pluggable field / sim driver.
///
/// Network drivers run on non-RT workers only (KD-5a). Local GPIO may run in-RT
/// when WCET-bounded.
pub trait IoDriver: Send {
    /// Stable driver instance name.
    fn name(&self) -> &str;

    /// Start polling / claim hardware.
    ///
    /// # Errors
    /// Returns [`IoError`] when the driver cannot start.
    fn start(&mut self) -> Result<(), IoError>;

    /// Stop and release resources.
    fn stop(&mut self);

    /// Poll inputs into `out` (non-RT or in-RT per driver class).
    ///
    /// # Errors
    /// Returns [`IoError`] on hard failure; soft faults may set quality Bad.
    fn poll_inputs(&mut self, out: &mut InputUpdate) -> Result<(), IoError>;

    /// Apply outputs; must honor `force_safe` / safe_state semantics.
    ///
    /// # Errors
    /// Returns [`IoError`] when outputs cannot be applied.
    fn apply_outputs(&mut self, image: &OutputImage) -> Result<(), IoError>;

    /// Diagnostics snapshot.
    fn diagnostics(&self) -> DriverDiag;
}
