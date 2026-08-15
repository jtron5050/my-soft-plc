//! Simulation I/O driver for CI, desktop, and `mode=SIM`.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use plc_io::{DriverDiag, InputUpdate, IoDriver, IoError, OutputImage, PlcValue};
use plc_types::Quality;

/// In-memory sim driver: tests inject inputs and quality; capture last outputs.
#[derive(Debug)]
pub struct SimDriver {
    name: String,
    n_inputs: usize,
    n_outputs: usize,
    running: bool,
    /// Injected input values (defaults false/0).
    inputs: Vec<PlcValue>,
    /// Injected input quality.
    input_quality: Vec<Quality>,
    /// Last applied outputs.
    pub last_outputs: Vec<PlcValue>,
    /// Last force_safe flag seen on apply.
    pub last_force_safe: bool,
    seq: u64,
    fail_count: u32,
    /// Optional per-slot quality overrides by index.
    quality_overrides: BTreeMap<usize, Quality>,
}

impl SimDriver {
    /// Create a sim driver with fixed channel counts.
    #[must_use]
    pub fn new(name: impl Into<String>, n_inputs: usize, n_outputs: usize) -> Self {
        Self {
            name: name.into(),
            n_inputs,
            n_outputs,
            running: false,
            inputs: vec![PlcValue::Bool(false); n_inputs],
            input_quality: vec![Quality::Good; n_inputs],
            last_outputs: vec![PlcValue::Bool(false); n_outputs],
            last_force_safe: false,
            seq: 0,
            fail_count: 0,
            quality_overrides: BTreeMap::new(),
        }
    }

    /// Inject a BOOL/typed input value.
    pub fn set_input(&mut self, idx: usize, value: PlcValue) {
        if let Some(slot) = self.inputs.get_mut(idx) {
            *slot = value;
        }
    }

    /// Inject quality for an input (e.g. Bad for fault injection).
    pub fn set_input_quality(&mut self, idx: usize, quality: Quality) {
        if let Some(slot) = self.input_quality.get_mut(idx) {
            *slot = quality;
        }
        self.quality_overrides.insert(idx, quality);
    }
}

impl IoDriver for SimDriver {
    fn name(&self) -> &str {
        &self.name
    }

    fn start(&mut self) -> Result<(), IoError> {
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) {
        self.running = false;
    }

    fn poll_inputs(&mut self, out: &mut InputUpdate) -> Result<(), IoError> {
        if !self.running {
            return Err(IoError::NotReady(self.name.clone()));
        }
        self.seq = self.seq.wrapping_add(1);
        out.values.clear();
        out.quality.clear();
        out.values.extend_from_slice(&self.inputs);
        for (i, q) in self.input_quality.iter().enumerate() {
            let q = self.quality_overrides.get(&i).copied().unwrap_or(*q);
            out.quality.push(q);
        }
        // Ensure parallel lengths.
        while out.quality.len() < out.values.len() {
            out.quality.push(Quality::Good);
        }
        out.seq = self.seq;
        let _ = self.n_inputs;
        Ok(())
    }

    fn apply_outputs(&mut self, image: &OutputImage) -> Result<(), IoError> {
        if !self.running {
            return Err(IoError::NotReady(self.name.clone()));
        }
        self.last_force_safe = image.force_safe;
        self.last_outputs.clear();
        if image.force_safe {
            // De-energize: all false / zero for sim.
            self.last_outputs.resize(
                self.n_outputs.max(image.values.len()),
                PlcValue::Bool(false),
            );
            for slot in &mut self.last_outputs {
                *slot = PlcValue::Bool(false);
            }
        } else {
            self.last_outputs.clone_from(&image.values);
            if self.last_outputs.len() < self.n_outputs {
                self.last_outputs
                    .resize(self.n_outputs, PlcValue::Bool(false));
            }
        }
        Ok(())
    }

    fn diagnostics(&self) -> DriverDiag {
        DriverDiag {
            status: if self.running {
                "running".into()
            } else {
                "stopped".into()
            },
            fail_count: self.fail_count,
            last_seq: self.seq,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_and_apply() {
        let mut d = SimDriver::new("sim0", 2, 1);
        d.start().unwrap();
        d.set_input(0, PlcValue::Bool(true));
        d.set_input_quality(1, Quality::Bad);
        let mut up = InputUpdate::zeros(0);
        d.poll_inputs(&mut up).unwrap();
        assert_eq!(up.values[0], PlcValue::Bool(true));
        assert_eq!(up.quality[1], Quality::Bad);

        d.apply_outputs(&OutputImage {
            values: vec![PlcValue::Bool(true)],
            force_safe: false,
        })
        .unwrap();
        assert_eq!(d.last_outputs[0], PlcValue::Bool(true));

        d.apply_outputs(&OutputImage {
            values: vec![PlcValue::Bool(true)],
            force_safe: true,
        })
        .unwrap();
        assert_eq!(d.last_outputs[0], PlcValue::Bool(false));
        assert!(d.last_force_safe);
    }
}
