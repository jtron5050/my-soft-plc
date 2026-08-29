//! Telemetry / MQTT errors.

use thiserror::Error;

use plc_types::PlcError;

/// MQTT Sparkplug publisher failure.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TelemetryError {
    /// Configuration refused (missing broker, empty ids, bad catalog).
    #[error("telemetry config: {0}")]
    Config(String),
    /// MQTT client request channel is full; the publisher dropped this batch.
    #[error("mqtt client backpressured")]
    Backpressured,
    /// MQTT client / session error.
    #[error("mqtt: {0}")]
    Mqtt(String),
    /// Sparkplug protobuf encode/decode failure.
    #[error("protobuf: {0}")]
    Protobuf(String),
}

impl TelemetryError {
    /// Configuration error.
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    /// MQTT error.
    pub fn mqtt(msg: impl Into<String>) -> Self {
        Self::Mqtt(msg.into())
    }

    /// Protobuf error.
    pub fn protobuf(msg: impl Into<String>) -> Self {
        Self::Protobuf(msg.into())
    }
}

impl From<TelemetryError> for PlcError {
    fn from(err: TelemetryError) -> Self {
        match err {
            TelemetryError::Config(m) => Self::Config(m),
            other => Self::Internal(other.to_string()),
        }
    }
}
