//! Scan driver that keeps sim inject and overlays Modbus slots.

use plc_io::{DriverDiag, InputUpdate, IoDriver, IoError, OutputImage, OutputModuleState};
use plc_io_modbus::ModbusBridge;
use plc_io_sim::SharedSim;

/// Sim image plus a Modbus snapshot bridge.
///
/// `poll_inputs` fills from the sim driver, then copies Modbus slots unless the
/// mode gate is SIM. Field writes are enqueued on the bridge; the worker drops
/// them while the gate is SIM.
pub struct RoutedDriver {
    sim: SharedSim,
    bridge: ModbusBridge,
}

impl RoutedDriver {
    /// Pair an already-started sim handle with a Modbus bridge.
    #[must_use]
    pub fn new(sim: SharedSim, bridge: ModbusBridge) -> Self {
        Self { sim, bridge }
    }
}

impl IoDriver for RoutedDriver {
    fn name(&self) -> &'static str {
        "modbus_tcp"
    }

    fn start(&mut self) -> Result<(), IoError> {
        self.bridge.start()
    }

    fn stop(&mut self) {
        self.bridge.stop();
    }

    fn poll_inputs(&mut self, out: &mut InputUpdate) -> Result<(), IoError> {
        self.sim.poll_inputs(out)?;
        self.bridge.poll_inputs(out)
    }

    fn apply_outputs(&mut self, image: &OutputImage) -> Result<(), IoError> {
        self.sim.apply_outputs(image)?;
        self.bridge.apply_outputs(image)
    }

    fn diagnostics(&self) -> DriverDiag {
        self.bridge.diagnostics()
    }

    fn fill_output_module_state(&self, into: &mut [OutputModuleState]) -> bool {
        self.bridge.fill_output_module_state(into)
    }
}
