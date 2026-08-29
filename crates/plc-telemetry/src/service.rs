//! Tokio worker that owns the MQTT event loop.

use std::time::Duration;

use plc_config::DeviceConfig;
use plc_scan::{ScanHandle, TelemetrySource};
use rumqttc::v5::{Event, Incoming};

use crate::broker::{parse_broker_url, BrokerAddr};
use crate::catalog::TagCatalog;
use crate::clock::SystemWallClock;
use crate::error::TelemetryError;
use crate::mqtt::{connect_client, mqtt_options, RumqttTransport};
use crate::publisher::Publisher;
use crate::topics::TopicIds;

/// Library entry point for PR-14: spawn `run` on a tokio worker.
pub struct TelemetryService {
    enabled: bool,
    broker: Option<BrokerAddr>,
    client_id: String,
    publisher: Publisher<RumqttTransport, SystemWallClock, ScanHandle>,
}

impl TelemetryService {
    /// Build from device config. Empty `broker_url` is an error only when enabled.
    pub fn from_config(
        cfg: &DeviceConfig,
        source: TelemetrySource,
        handle: ScanHandle,
    ) -> Result<Self, TelemetryError> {
        let tel = &cfg.telemetry;
        let ids = TopicIds::new(
            tel.group_id.clone(),
            cfg.device.id.clone(),
            tel.device_id.clone(),
        )?;
        let broker = if tel.enabled {
            if tel.broker_url.trim().is_empty() {
                return Err(TelemetryError::config(
                    "telemetry.broker_url must be non-empty when telemetry.enabled",
                ));
            }
            Some(parse_broker_url(&tel.broker_url)?)
        } else {
            None
        };
        // Placeholder client; replaced on `run` before CONNECT.
        let dummy = mqtt_options(
            &cfg.device.id,
            broker.as_ref().unwrap_or(&BrokerAddr {
                host: "127.0.0.1".into(),
                port: 1883,
                tls: false,
            }),
            &ids.ndeath(),
            &bytes::Bytes::new(),
        )?;
        let (client, _eventloop) = connect_client(dummy);
        let mut publisher = Publisher::new(
            ids,
            source,
            RumqttTransport::new(client),
            SystemWallClock,
            handle,
        );
        publisher.set_catalog(TagCatalog::default());
        Ok(Self {
            enabled: tel.enabled,
            broker,
            client_id: cfg.device.id.clone(),
            publisher,
        })
    }

    /// Arm device metrics (PR-14 calls this after epoch activate).
    pub fn set_catalog(&mut self, catalog: TagCatalog) {
        self.publisher.set_catalog(catalog);
    }

    /// Drive MQTT until the future is dropped. No-op when telemetry is disabled.
    pub async fn run(mut self) -> Result<(), TelemetryError> {
        if !self.enabled {
            return Ok(());
        }
        let broker = self
            .broker
            .clone()
            .ok_or_else(|| TelemetryError::config("missing broker"))?;
        loop {
            self.publisher.prepare_connect();
            let will_topic = self.publisher.ndeath_topic();
            let will = self.publisher.ndeath_bytes();
            let opts = mqtt_options(&self.client_id, &broker, &will_topic, &will)?;
            let (client, mut eventloop) = connect_client(opts);
            self.publisher
                .replace_transport(RumqttTransport::new(client));
            self.publisher.on_connected()?;

            loop {
                tokio::select! {
                    ev = eventloop.poll() => {
                        match ev {
                            Ok(Event::Incoming(Incoming::Publish(p))) => {
                                let topic = std::str::from_utf8(p.topic.as_ref())
                                    .unwrap_or("");
                                self.publisher.handle_incoming(topic, p.payload.as_ref())?;
                            }
                            Ok(_) => {}
                            Err(_) => {
                                break;
                            }
                        }
                    }
                    () = tokio::time::sleep(Duration::from_millis(10)) => {
                        self.publisher.drain()?;
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}
