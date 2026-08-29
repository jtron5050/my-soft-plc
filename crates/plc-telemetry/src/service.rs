//! Tokio worker that owns the MQTT event loop.

use std::time::Duration;

use plc_config::DeviceConfig;
use plc_scan::{ScanHandle, TelemetrySource};
use rumqttc::v5::mqttbytes::v5::ConnectReturnCode;
use rumqttc::v5::{Event, Incoming};

use crate::broker::{parse_broker_url, BrokerAddr};
use crate::catalog::TagCatalog;
use crate::clock::SystemWallClock;
use crate::error::TelemetryError;
use crate::mqtt::{connect_client, mqtt_options, RumqttTransport};
use crate::publisher::Publisher;
use crate::topics::TopicIds;

/// Cloneable control plane for a running [`TelemetryService`] worker.
///
/// Clone this before spawning [`TelemetryService::run`] so PR-14 can arm or
/// replace the catalog after epoch activate.
#[derive(Clone, Debug)]
pub struct TelemetryHandle {
    catalog_tx: tokio::sync::mpsc::UnboundedSender<TagCatalog>,
}

impl TelemetryHandle {
    /// Arm / replace the device metric catalog on the MQTT worker.
    pub fn set_catalog(&self, catalog: TagCatalog) {
        let _ = self.catalog_tx.send(catalog);
    }
}

/// Library entry point for PR-14: spawn `run` on a tokio worker.
pub struct TelemetryService {
    enabled: bool,
    broker: Option<BrokerAddr>,
    client_id: String,
    publisher: Publisher<RumqttTransport, SystemWallClock, ScanHandle>,
    catalog_tx: tokio::sync::mpsc::UnboundedSender<TagCatalog>,
    catalog_rx: tokio::sync::mpsc::UnboundedReceiver<TagCatalog>,
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
        publisher.set_catalog(TagCatalog::default())?;
        let (catalog_tx, catalog_rx) = tokio::sync::mpsc::unbounded_channel();
        Ok(Self {
            enabled: tel.enabled,
            broker,
            client_id: cfg.device.id.clone(),
            publisher,
            catalog_tx,
            catalog_rx,
        })
    }

    /// Cloneable handle; call before [`Self::run`] so catalog updates still work.
    #[must_use]
    pub fn handle(&self) -> TelemetryHandle {
        TelemetryHandle {
            catalog_tx: self.catalog_tx.clone(),
        }
    }

    /// Arm device metrics (PR-14 calls this after epoch activate).
    ///
    /// After [`Self::run`] is spawned, use [`TelemetryHandle::set_catalog`]
    /// instead — this method requires `&mut self`.
    pub fn set_catalog(&mut self, catalog: TagCatalog) {
        let _ = self.publisher.set_catalog(catalog);
    }

    /// Drive MQTT until the future is dropped. No-op when telemetry is disabled.
    ///
    /// Subscribe / NBIRTH / DBIRTH run only after a successful CONNACK.
    /// `drain` is a no-op until then so the scan SPSC is not consumed.
    /// `bdSeq` increments only after a session that reached CONNACK ends.
    pub async fn run(mut self) -> Result<(), TelemetryError> {
        if !self.enabled {
            return Ok(());
        }
        let broker = self
            .broker
            .clone()
            .ok_or_else(|| TelemetryError::config("missing broker"))?;
        let mut catalog_rx = self.catalog_rx;
        let mut catalog_open = true;
        self.publisher.prepare_connect();
        loop {
            while let Ok(catalog) = catalog_rx.try_recv() {
                self.publisher.set_catalog(catalog)?;
            }
            let will_topic = self.publisher.ndeath_topic();
            let will = self.publisher.ndeath_bytes();
            let opts = mqtt_options(&self.client_id, &broker, &will_topic, &will)?;
            let (client, mut eventloop) = connect_client(opts);
            self.publisher
                .replace_transport(RumqttTransport::new(client));
            let mut session_live = false;
            loop {
                tokio::select! {
                    ev = eventloop.poll() => {
                        match ev {
                            Ok(Event::Incoming(Incoming::ConnAck(ack))) => {
                                if ack.code == ConnectReturnCode::Success && !session_live {
                                    self.publisher.on_connected()?;
                                    session_live = true;
                                }
                            }
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
                    catalog = catalog_rx.recv(), if catalog_open => {
                        match catalog {
                            Some(catalog) => self.publisher.set_catalog(catalog)?,
                            None => catalog_open = false,
                        }
                    }
                    () = tokio::time::sleep(Duration::from_millis(10)) => {
                        self.publisher.drain()?;
                    }
                }
            }
            if session_live {
                self.publisher.prepare_connect();
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}
