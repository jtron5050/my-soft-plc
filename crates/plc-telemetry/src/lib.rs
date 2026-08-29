//! MQTT 5 + Sparkplug B 3.0 telemetry (architecture PR-13).
//!
//! Non-RT. Consumes [`plc_scan::TelemetrySource`] and never blocks the scan
//! thread. No WebSocket server (KD-21).

#![deny(unsafe_code)]

mod broker;
mod catalog;
mod clock;
mod error;
mod mqtt;
mod protobuf;
mod publisher;
mod service;
mod session;
mod topics;
mod transport;
mod types;

pub use broker::{parse_broker_url, BrokerAddr};
pub use catalog::{CatalogEntry, CatalogTag, TagCatalog};
pub use clock::{MockWallClock, SystemWallClock, WallClock};
pub use error::TelemetryError;
pub use protobuf::{is_rebirth_command, Metric, MetricValue, Payload, Property};
pub use publisher::{ConstMode, ModeSource, Publisher};
pub use service::{TelemetryHandle, TelemetryService};
pub use session::{SessionState, METRIC_BDSEQ, METRIC_DROPS, METRIC_MODE, METRIC_REBIRTH};
pub use topics::{TopicIds, NAMESPACE};
pub use transport::{PublishedFrame, RecordingTransport, Transport, SPARKPLUG_QOS};
pub use types::{
    quality_code, value_to_sparkplug, MetricType, QUALITY_BAD, QUALITY_GOOD, QUALITY_UNCERTAIN,
    SP_BOOLEAN, SP_FLOAT, SP_INT16, SP_INT32, SP_INT64, SP_STRING, SP_UINT64,
};
