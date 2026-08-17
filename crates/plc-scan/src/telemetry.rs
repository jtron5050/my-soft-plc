//! `TelemetrySource` — RT → non-RT sample API for PR-13.

use plc_io::PlcValue;
use plc_types::Quality;

use crate::spsc::{channel, SpscConsumer, SpscProducer};

/// One process-image sample published by the scan thread.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TelemetrySample {
    /// Temporary alias (image slot index). PR-13 remaps to Sparkplug aliases.
    pub alias: u32,
    /// Process-image slot this sample came from.
    pub tag_hint: u32,
    /// Sampled value.
    pub value: PlcValue,
    /// Quality plane at publish time.
    pub quality: Quality,
    /// Maintenance force was active for this output.
    pub forced: bool,
    /// Monotonic sample time in milliseconds (wall stamp is PR-13 / KD-19).
    pub now_ms: u64,
    /// True when the slot is an input (`%I`); false for `%Q`.
    pub is_input: bool,
}

impl Default for TelemetrySample {
    fn default() -> Self {
        Self {
            alias: 0,
            tag_hint: 0,
            value: PlcValue::Bool(false),
            quality: Quality::Good,
            forced: false,
            now_ms: 0,
            is_input: false,
        }
    }
}

/// Producer half held by [`crate::ScanEngine`].
#[derive(Clone)]
pub struct TelemetrySink {
    tx: SpscProducer<TelemetrySample>,
}

impl TelemetrySink {
    /// Non-blocking publish.
    pub fn publish(&self, sample: TelemetrySample) {
        self.tx.push_drop_oldest(sample);
    }
}

/// Consumer half for `plc-telemetry` (PR-13).
#[derive(Clone)]
pub struct TelemetrySource {
    rx: SpscConsumer<TelemetrySample>,
}

impl TelemetrySource {
    /// Pop one sample; `None` if the ring is empty.
    #[must_use]
    pub fn try_recv(&self) -> Option<TelemetrySample> {
        self.rx.try_recv()
    }

    /// Cumulative dropped samples (never blocks the scan).
    #[must_use]
    pub fn drops(&self) -> u64 {
        self.rx.drops()
    }
}

/// Allocate a telemetry ring with `capacity` slots.
#[must_use]
pub fn telemetry_channel(capacity: usize) -> (TelemetrySink, TelemetrySource) {
    let (tx, rx) = channel(capacity);
    (TelemetrySink { tx }, TelemetrySource { rx })
}

/// Last-published tracker used for CoS / analog period (preallocated).
#[derive(Debug, Clone)]
pub struct PublishTrack {
    last_value: Option<PlcValue>,
    last_ms: u64,
    ever: bool,
}

impl PublishTrack {
    /// Empty tracker.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            last_value: None,
            last_ms: 0,
            ever: false,
        }
    }

    /// Whether this slot should be published now.
    #[must_use]
    pub fn should_publish(
        &self,
        value: PlcValue,
        now_ms: u64,
        is_bool: bool,
        analog_period_ms: u32,
        digital_cos_ms: u32,
    ) -> bool {
        if !self.ever {
            return true;
        }
        let changed = self.last_value != Some(value);
        let elapsed = now_ms.saturating_sub(self.last_ms);
        if is_bool {
            changed && elapsed >= u64::from(digital_cos_ms)
        } else {
            changed || elapsed >= u64::from(analog_period_ms)
        }
    }

    /// Record a publish.
    pub fn mark(&mut self, value: PlcValue, now_ms: u64) {
        self.last_value = Some(value);
        self.last_ms = now_ms;
        self.ever = true;
    }
}

impl Default for PublishTrack {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_always_publishes() {
        let t = PublishTrack::new();
        assert!(t.should_publish(PlcValue::Bool(false), 0, true, 500, 20));
    }

    #[test]
    fn bool_cos_respects_min_period() {
        let mut t = PublishTrack::new();
        t.mark(PlcValue::Bool(false), 0);
        assert!(!t.should_publish(PlcValue::Bool(true), 10, true, 500, 20));
        assert!(t.should_publish(PlcValue::Bool(true), 20, true, 500, 20));
        assert!(!t.should_publish(PlcValue::Bool(false), 30, true, 500, 20));
    }
}
