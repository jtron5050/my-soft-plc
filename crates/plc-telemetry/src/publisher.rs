//! Drain `TelemetrySource` into Sparkplug frames (no rumqttc).

use bytes::Bytes;
use plc_scan::{ScanHandle, TelemetrySource};
use plc_types::{OperatingMode, Quality};

use crate::catalog::TagCatalog;
use crate::clock::WallClock;
use crate::error::TelemetryError;
use crate::protobuf::{is_rebirth_command, Payload};
use crate::session::SessionState;
use crate::topics::TopicIds;
use crate::transport::{Transport, SPARKPLUG_QOS};
use crate::types::publish_quality;

/// Non-RT source of operator mode for `SYSTEM/Mode`.
pub trait ModeSource: Send + Sync {
    /// Current operator mode.
    fn mode(&self) -> OperatingMode;
}

impl ModeSource for ScanHandle {
    fn mode(&self) -> OperatingMode {
        ScanHandle::mode(self)
    }
}

/// Fixed mode for unit tests.
#[derive(Debug, Clone, Copy)]
pub struct ConstMode(pub OperatingMode);

impl ModeSource for ConstMode {
    fn mode(&self) -> OperatingMode {
        self.0
    }
}

/// Testable Sparkplug publisher.
pub struct Publisher<T, C, M> {
    ids: TopicIds,
    source: TelemetrySource,
    transport: T,
    clock: C,
    mode: M,
    session: SessionState,
    mqtt_drops: u64,
    born: bool,
}

impl<T: Transport, C: WallClock, M: ModeSource> Publisher<T, C, M> {
    /// Construct with an empty catalog (set later at arm).
    pub fn new(ids: TopicIds, source: TelemetrySource, transport: T, clock: C, mode: M) -> Self {
        Self {
            ids,
            source,
            transport,
            clock,
            mode,
            session: SessionState::new(),
            mqtt_drops: 0,
            born: false,
        }
    }

    /// Access the transport (tests inspect [`crate::RecordingTransport`]).
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Mutable transport (tests flip `publish_full`).
    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Replace the MQTT client after reconnect.
    pub fn replace_transport(&mut self, transport: T) {
        self.transport = transport;
        self.born = false;
    }

    /// MQTT-side dropped batches (client full).
    #[must_use]
    pub fn mqtt_drops(&self) -> u64 {
        self.mqtt_drops
    }

    /// Scan-side SPSC drops (never blocks the scan).
    #[must_use]
    pub fn scan_drops(&self) -> u64 {
        self.source.drops()
    }

    /// Arm / replace the device metric catalog.
    ///
    /// While born, a non-empty previous catalog is retired with DDEATH, then
    /// the new catalog is published as DBIRTH (empty catalog skips DBIRTH).
    pub fn set_catalog(&mut self, catalog: TagCatalog) -> Result<(), TelemetryError> {
        if self.born && !self.session.catalog().is_empty() {
            self.publish_ddeath()?;
        }
        self.session.set_catalog(catalog);
        if self.born {
            self.publish_dbirth()?;
        }
        Ok(())
    }

    /// True after a successful CONNACK birth sequence.
    #[must_use]
    pub fn is_born(&self) -> bool {
        self.born
    }

    /// Increment `bdSeq` (except first session) before CONNECT/Will.
    pub fn prepare_connect(&mut self) {
        self.session.prepare_connect();
        self.born = false;
    }

    /// Current `bdSeq` for the Will / NBIRTH pair.
    #[must_use]
    pub fn bd_seq(&self) -> u64 {
        self.session.bd_seq()
    }

    /// Encoded NDEATH Will payload for the current `bdSeq`.
    #[must_use]
    pub fn ndeath_bytes(&self) -> Bytes {
        Bytes::from(self.session.ndeath(self.clock.unix_ms()).encode())
    }

    /// NDEATH topic (MQTT Will).
    #[must_use]
    pub fn ndeath_topic(&self) -> String {
        self.ids.ndeath()
    }

    /// NCMD topic to subscribe.
    #[must_use]
    pub fn ncmd_topic(&self) -> String {
        self.ids.ncmd()
    }

    /// Subscribe NCMD, publish NBIRTH then DBIRTH.
    pub fn on_connected(&mut self) -> Result<(), TelemetryError> {
        self.try_send_sub()?;
        self.publish_nbirth()?;
        self.publish_dbirth()?;
        self.born = true;
        Ok(())
    }

    /// Drain the telemetry ring and publish NDATA/DDATA. Never waits on MQTT.
    /// No-op until [`Self::on_connected`] so the scan SPSC is not consumed
    /// before CONNACK.
    pub fn drain(&mut self) -> Result<(), TelemetryError> {
        if !self.born {
            return Ok(());
        }
        let mut samples = Vec::new();
        while let Some(s) = self.source.try_recv() {
            samples.push(s);
        }
        let ts = self.clock.unix_ms();
        let synced = self.clock.is_synchronized();
        let q = publish_quality(Quality::Good, synced);
        if let Some(payload) = self
            .session
            .ndata(ts, self.mode.mode(), self.source.drops(), q)
        {
            let topic = self.ids.ndata();
            self.send(&topic, &payload)?;
        }
        if !samples.is_empty() {
            if let Some(payload) = self.session.ddata(ts, &samples, synced) {
                let topic = self.ids.ddata();
                self.send(&topic, &payload)?;
            }
        }
        Ok(())
    }

    /// Handle an incoming MQTT publish (NCMD rebirth).
    pub fn handle_incoming(&mut self, topic: &str, payload: &[u8]) -> Result<(), TelemetryError> {
        if !self.ids.is_ncmd(topic) {
            return Ok(());
        }
        if is_rebirth_command(payload) {
            self.session.on_rebirth();
            self.publish_nbirth()?;
            self.publish_dbirth()?;
        }
        Ok(())
    }

    fn publish_nbirth(&mut self) -> Result<(), TelemetryError> {
        let ts = self.clock.unix_ms();
        let q = publish_quality(Quality::Good, self.clock.is_synchronized());
        let payload = self
            .session
            .nbirth(ts, self.mode.mode(), self.source.drops(), q);
        let topic = self.ids.nbirth();
        self.send(&topic, &payload)
    }

    fn publish_dbirth(&mut self) -> Result<(), TelemetryError> {
        let ts = self.clock.unix_ms();
        if let Some(payload) = self.session.dbirth(ts, self.clock.is_synchronized()) {
            let topic = self.ids.dbirth();
            self.send(&topic, &payload)?;
        }
        Ok(())
    }

    fn publish_ddeath(&mut self) -> Result<(), TelemetryError> {
        let ts = self.clock.unix_ms();
        let payload = self.session.ddeath(ts);
        let topic = self.ids.ddeath();
        self.send(&topic, &payload)
    }

    fn try_send_sub(&mut self) -> Result<(), TelemetryError> {
        match self.transport.subscribe(&self.ids.ncmd(), SPARKPLUG_QOS) {
            Ok(()) => Ok(()),
            Err(TelemetryError::Backpressured) => {
                self.mqtt_drops = self.mqtt_drops.saturating_add(1);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    fn send(&mut self, topic: &str, payload: &Payload) -> Result<(), TelemetryError> {
        let bytes = Bytes::from(payload.encode());
        match self.transport.publish(topic, SPARKPLUG_QOS, false, bytes) {
            Ok(()) => Ok(()),
            Err(TelemetryError::Backpressured) => {
                self.mqtt_drops = self.mqtt_drops.saturating_add(1);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
}
