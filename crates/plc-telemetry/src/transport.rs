//! MQTT publish/subscribe abstraction (real client vs recording fake).

use bytes::Bytes;

use crate::error::TelemetryError;

/// Frozen Sparkplug QoS (architecture contract; not Sparkplug TCK QoS 0).
pub const SPARKPLUG_QOS: u8 = 1;

/// Non-blocking MQTT operations used by [`crate::publisher::Publisher`].
pub trait Transport: Send {
    /// Publish one packet. Return [`TelemetryError::Backpressured`] instead of
    /// blocking when the client cannot accept the message.
    fn publish(
        &mut self,
        topic: &str,
        qos: u8,
        retain: bool,
        payload: Bytes,
    ) -> Result<(), TelemetryError>;

    /// Subscribe (NCMD). Must not block the scan thread; this runs on tokio.
    fn subscribe(&mut self, topic: &str, qos: u8) -> Result<(), TelemetryError>;
}

/// One recorded publish (tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedFrame {
    /// Topic.
    pub topic: String,
    /// QoS.
    pub qos: u8,
    /// Retain flag.
    pub retain: bool,
    /// Raw Sparkplug payload.
    pub payload: Bytes,
}

/// In-memory transport for unit tests (no broker).
#[derive(Debug, Default)]
pub struct RecordingTransport {
    /// Captured publishes, in order.
    pub publishes: Vec<PublishedFrame>,
    /// Captured subscriptions.
    pub subscriptions: Vec<(String, u8)>,
    /// When true, `publish` returns [`TelemetryError::Backpressured`].
    pub publish_full: bool,
}

impl Transport for RecordingTransport {
    fn publish(
        &mut self,
        topic: &str,
        qos: u8,
        retain: bool,
        payload: Bytes,
    ) -> Result<(), TelemetryError> {
        if self.publish_full {
            return Err(TelemetryError::Backpressured);
        }
        self.publishes.push(PublishedFrame {
            topic: topic.to_string(),
            qos,
            retain,
            payload,
        });
        Ok(())
    }

    fn subscribe(&mut self, topic: &str, qos: u8) -> Result<(), TelemetryError> {
        self.subscriptions.push((topic.to_string(), qos));
        Ok(())
    }
}

impl RecordingTransport {
    /// Decode payload of publishes whose topic ends with `suffix` (e.g. `/NBIRTH/`).
    #[must_use]
    pub fn payloads_on(&self, topic: &str) -> Vec<&PublishedFrame> {
        self.publishes.iter().filter(|f| f.topic == topic).collect()
    }
}
