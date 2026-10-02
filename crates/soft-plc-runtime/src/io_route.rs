//! Scan driver that keeps sim inject and overlays field slots.

use plc_io::{DriverDiag, InputUpdate, IoDriver, IoError, OutputImage, OutputModuleState};
use plc_io_gpio::GpioDriver;
use plc_io_modbus::ModbusBridge;
use plc_io_sim::SharedSim;

/// Sim image plus optional Modbus and GPIO drivers.
///
/// `poll_inputs` fills from the sim driver, then copies Modbus slots, then
/// GPIO slots, unless that driver's mode gate is SIM. Field writes go to the
/// Modbus worker and, for GPIO, to the scan-thread ioctl.
pub struct RoutedDriver {
    sim: SharedSim,
    bridge: Option<ModbusBridge>,
    gpio: Option<GpioDriver>,
}

impl RoutedDriver {
    /// Pair an already-started sim handle with the field drivers that are configured.
    #[must_use]
    pub fn new(sim: SharedSim, bridge: Option<ModbusBridge>, gpio: Option<GpioDriver>) -> Self {
        Self { sim, bridge, gpio }
    }
}

impl IoDriver for RoutedDriver {
    fn name(&self) -> &'static str {
        match (&self.bridge, &self.gpio) {
            (Some(_), Some(_)) => "field",
            (Some(_), None) => "modbus_tcp",
            (None, Some(_)) => "gpio",
            (None, None) => "sim",
        }
    }

    fn start(&mut self) -> Result<(), IoError> {
        if let Some(gpio) = &mut self.gpio {
            gpio.start()?;
        }
        if let Some(bridge) = &mut self.bridge {
            if let Err(err) = bridge.start() {
                if let Some(gpio) = &mut self.gpio {
                    gpio.stop();
                }
                return Err(err);
            }
        }
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(bridge) = &mut self.bridge {
            bridge.stop();
        }
        if let Some(gpio) = &mut self.gpio {
            gpio.stop();
        }
    }

    fn poll_inputs(&mut self, out: &mut InputUpdate) -> Result<(), IoError> {
        self.sim.poll_inputs(out)?;
        if let Some(bridge) = &mut self.bridge {
            bridge.poll_inputs(out)?;
        }
        if let Some(gpio) = &mut self.gpio {
            gpio.poll_inputs(out)?;
        }
        Ok(())
    }

    fn apply_outputs(&mut self, image: &OutputImage) -> Result<(), IoError> {
        self.sim.apply_outputs(image)?;
        if let Some(bridge) = &mut self.bridge {
            bridge.apply_outputs(image)?;
        }
        if let Some(gpio) = &mut self.gpio {
            gpio.apply_outputs(image)?;
        }
        Ok(())
    }

    fn diagnostics(&self) -> DriverDiag {
        match (&self.bridge, &self.gpio) {
            (Some(bridge), Some(gpio)) => {
                let modbus = bridge.diagnostics();
                let pins = gpio.diagnostics();
                DriverDiag {
                    status: format!("modbus: {}; gpio: {}", modbus.status, pins.status),
                    fail_count: modbus.fail_count.saturating_add(pins.fail_count),
                    last_seq: modbus.last_seq.max(pins.last_seq),
                }
            }
            (Some(bridge), None) => bridge.diagnostics(),
            (None, Some(gpio)) => gpio.diagnostics(),
            (None, None) => self.sim.diagnostics(),
        }
    }

    fn fill_output_module_state(&self, into: &mut [OutputModuleState]) -> bool {
        let mut filled = false;
        if let Some(bridge) = &self.bridge {
            filled |= bridge.fill_output_module_state(into);
        }
        if let Some(gpio) = &self.gpio {
            filled |= gpio.fill_output_module_state(into);
        }
        filled
    }
}
