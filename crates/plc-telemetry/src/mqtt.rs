//! rumqttc v5 client wrapper.

use std::time::Duration;

use bytes::Bytes;
use rumqttc::v5::mqttbytes::v5::LastWill;
use rumqttc::v5::mqttbytes::QoS;
use rumqttc::v5::{AsyncClient, ClientError, EventLoop, MqttOptions};

use crate::broker::BrokerAddr;
use crate::error::TelemetryError;
use crate::transport::Transport;

/// MQTT 5 session expiry (architecture frozen default).
pub const SESSION_EXPIRY_SECS: u32 = 3600;
/// Keep-alive.
pub const KEEP_ALIVE: Duration = Duration::from_secs(30);

/// rumqttc transport using `try_publish` (never blocks).
pub struct RumqttTransport {
    client: AsyncClient,
}

impl RumqttTransport {
    /// Wrap an async client.
    #[must_use]
    pub fn new(client: AsyncClient) -> Self {
        Self { client }
    }
}

impl Transport for RumqttTransport {
    fn publish(
        &mut self,
        topic: &str,
        qos: u8,
        retain: bool,
        payload: Bytes,
    ) -> Result<(), TelemetryError> {
        let qos = qos_from_u8(qos);
        self.client
            .try_publish(topic, qos, retain, payload)
            .map_err(map_client_err)
    }

    fn subscribe(&mut self, topic: &str, qos: u8) -> Result<(), TelemetryError> {
        self.client
            .try_subscribe(topic, qos_from_u8(qos))
            .map_err(map_client_err)
    }
}

fn qos_from_u8(qos: u8) -> QoS {
    match qos {
        0 => QoS::AtMostOnce,
        2 => QoS::ExactlyOnce,
        _ => QoS::AtLeastOnce,
    }
}

fn map_client_err(err: ClientError) -> TelemetryError {
    match err {
        ClientError::TryRequest(_) => TelemetryError::Backpressured,
        ClientError::Request(req) => TelemetryError::mqtt(format!("mqtt request: {req:?}")),
    }
}

/// Build MQTT 5 options: clean start false, session expiry 3600 s, Will.
pub fn mqtt_options(
    client_id: &str,
    broker: &BrokerAddr,
    will_topic: &str,
    will_payload: &Bytes,
) -> Result<MqttOptions, TelemetryError> {
    if client_id.is_empty() {
        return Err(TelemetryError::config("MQTT client id must be non-empty"));
    }
    let mut opts = MqttOptions::new(client_id, &broker.host, broker.port);
    opts.set_clean_start(false);
    opts.set_keep_alive(KEEP_ALIVE);
    opts.set_session_expiry_interval(Some(SESSION_EXPIRY_SECS));
    let will = LastWill::new(
        will_topic,
        will_payload.as_ref(),
        QoS::AtLeastOnce,
        false,
        None,
    );
    opts.set_last_will(will);
    if broker.tls {
        opts.set_transport(rumqttc::Transport::tls_with_default_config());
    }
    Ok(opts)
}

/// Open a client + event loop. Cap in-flight requests so `try_publish` can fail
/// full instead of growing without bound.
pub fn connect_client(opts: MqttOptions) -> (AsyncClient, EventLoop) {
    AsyncClient::new(opts, 16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broker::parse_broker_url;

    #[test]
    fn session_knobs() {
        let broker = parse_broker_url("mqtt://127.0.0.1:1883").unwrap();
        let opts = mqtt_options(
            "softplc-01",
            &broker,
            "spBv1.0/g/NDEATH/n",
            &Bytes::from_static(&[1]),
        )
        .unwrap();
        assert!(!opts.clean_start());
        assert_eq!(opts.session_expiry_interval(), Some(SESSION_EXPIRY_SECS));
        assert_eq!(opts.keep_alive(), KEEP_ALIVE);
        assert!(opts.last_will().is_some());
    }
}
