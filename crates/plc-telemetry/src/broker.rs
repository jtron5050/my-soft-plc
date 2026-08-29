//! MQTT broker URL parsing (`mqtt://` / `mqtts://`).

use crate::error::TelemetryError;

/// Parsed broker endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokerAddr {
    /// Hostname or IPv4 literal.
    pub host: String,
    /// TCP port.
    pub port: u16,
    /// Use TLS (`mqtts://`).
    pub tls: bool,
}

/// Parse `mqtt://host[:port]` or `mqtts://host[:port]`.
pub fn parse_broker_url(url: &str) -> Result<BrokerAddr, TelemetryError> {
    let url = url.trim();
    let (tls, rest) = if let Some(rest) = url.strip_prefix("mqtts://") {
        (true, rest)
    } else if let Some(rest) = url.strip_prefix("mqtt://") {
        (false, rest)
    } else {
        return Err(TelemetryError::config(
            "telemetry.broker_url must start with mqtt:// or mqtts://",
        ));
    };
    let rest = rest.trim_end_matches('/');
    if rest.is_empty() || rest.contains('/') {
        return Err(TelemetryError::config(
            "telemetry.broker_url host must not include a path",
        ));
    }
    let (host, port) = if let Some((h, p)) = rest.rsplit_once(':') {
        if h.is_empty() {
            return Err(TelemetryError::config("telemetry.broker_url host is empty"));
        }
        let port: u16 = p
            .parse()
            .map_err(|_| TelemetryError::config("telemetry.broker_url port must be 1–65535"))?;
        (h.to_string(), port)
    } else {
        (rest.to_string(), if tls { 8883 } else { 1883 })
    };
    if host.is_empty() {
        return Err(TelemetryError::config("telemetry.broker_url host is empty"));
    }
    Ok(BrokerAddr { host, port, tls })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mqtt_default_port() {
        let a = parse_broker_url("mqtt://127.0.0.1").unwrap();
        assert_eq!(
            a,
            BrokerAddr {
                host: "127.0.0.1".into(),
                port: 1883,
                tls: false,
            }
        );
    }

    #[test]
    fn mqtts_explicit_port() {
        let a = parse_broker_url("mqtts://broker:8884").unwrap();
        assert!(a.tls);
        assert_eq!(a.port, 8884);
    }

    #[test]
    fn rejects_http() {
        assert!(parse_broker_url("http://x").is_err());
    }
}
