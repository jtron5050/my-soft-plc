//! Sparkplug B topic layout (`spBv1.0`).

use crate::error::TelemetryError;

/// Sparkplug B namespace token.
pub const NAMESPACE: &str = "spBv1.0";

/// Identity tokens used in Sparkplug topics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicIds {
    /// `group_id` (`telemetry.group_id`).
    pub group_id: String,
    /// Edge node id (`device.id`).
    pub edge_node_id: String,
    /// Device id under the edge node (`telemetry.device_id`).
    pub device_id: String,
}

impl TopicIds {
    /// Build from config strings after trimming.
    pub fn new(
        group_id: impl Into<String>,
        edge_node_id: impl Into<String>,
        device_id: impl Into<String>,
    ) -> Result<Self, TelemetryError> {
        Ok(Self {
            group_id: parse_id("group_id", group_id)?,
            edge_node_id: parse_id("edge_node_id", edge_node_id)?,
            device_id: parse_id("device_id", device_id)?,
        })
    }

    fn node(&self, verb: &str) -> String {
        format!("{NAMESPACE}/{}/{verb}/{}", self.group_id, self.edge_node_id)
    }

    fn device(&self, verb: &str) -> String {
        format!(
            "{NAMESPACE}/{}/{verb}/{}/{}",
            self.group_id, self.edge_node_id, self.device_id
        )
    }

    /// `spBv1.0/{group}/NBIRTH/{edge}`.
    #[must_use]
    pub fn nbirth(&self) -> String {
        self.node("NBIRTH")
    }

    /// `spBv1.0/{group}/NDATA/{edge}`.
    #[must_use]
    pub fn ndata(&self) -> String {
        self.node("NDATA")
    }

    /// `spBv1.0/{group}/NDEATH/{edge}`.
    #[must_use]
    pub fn ndeath(&self) -> String {
        self.node("NDEATH")
    }

    /// `spBv1.0/{group}/NCMD/{edge}`.
    #[must_use]
    pub fn ncmd(&self) -> String {
        self.node("NCMD")
    }

    /// `spBv1.0/{group}/DBIRTH/{edge}/{device}`.
    #[must_use]
    pub fn dbirth(&self) -> String {
        self.device("DBIRTH")
    }

    /// `spBv1.0/{group}/DDATA/{edge}/{device}`.
    #[must_use]
    pub fn ddata(&self) -> String {
        self.device("DDATA")
    }

    /// `spBv1.0/{group}/DDEATH/{edge}/{device}`.
    #[must_use]
    pub fn ddeath(&self) -> String {
        self.device("DDEATH")
    }

    /// True when `topic` is this node's NCMD topic.
    #[must_use]
    pub fn is_ncmd(&self, topic: &str) -> bool {
        topic == self.ncmd()
    }
}

/// Sparkplug 3.0 forbids `/`, `+`, and `#` in identity tokens (MQTT wildcards).
fn parse_id(label: &str, raw: impl Into<String>) -> Result<String, TelemetryError> {
    let trimmed = raw.into().trim().to_string();
    if trimmed.is_empty() {
        return Err(TelemetryError::config(format!("{label} must be non-empty")));
    }
    if trimmed.contains('/') || trimmed.contains('+') || trimmed.contains('#') {
        return Err(TelemetryError::config(format!(
            "{label} must not contain '/', '+', or '#'"
        )));
    }
    Ok(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> TopicIds {
        TopicIds::new("plantA", "softplc-01", "line").unwrap()
    }

    #[test]
    fn node_topics_have_no_device_id() {
        let t = ids();
        assert_eq!(t.nbirth(), "spBv1.0/plantA/NBIRTH/softplc-01");
        assert_eq!(t.ndata(), "spBv1.0/plantA/NDATA/softplc-01");
        assert_eq!(t.ndeath(), "spBv1.0/plantA/NDEATH/softplc-01");
        assert_eq!(t.ncmd(), "spBv1.0/plantA/NCMD/softplc-01");
    }

    #[test]
    fn device_topics_include_device_id() {
        let t = ids();
        assert_eq!(t.dbirth(), "spBv1.0/plantA/DBIRTH/softplc-01/line");
        assert_eq!(t.ddata(), "spBv1.0/plantA/DDATA/softplc-01/line");
        assert_eq!(t.ddeath(), "spBv1.0/plantA/DDEATH/softplc-01/line");
    }

    #[test]
    fn slash_plus_hash_in_id_are_rejected() {
        assert!(TopicIds::new("a/b", "n", "d").is_err());
        assert!(TopicIds::new("a+b", "n", "d").is_err());
        assert!(TopicIds::new("a", "n#x", "d").is_err());
        assert!(TopicIds::new("#", "n", "d").is_err());
        assert!(TopicIds::new("+", "n", "d").is_err());
        assert!(TopicIds::new("a", "n", "d+").is_err());
    }

    #[test]
    fn new_trims_identity_tokens() {
        let t = TopicIds::new(" plantA ", " softplc-01 ", " line ").unwrap();
        assert_eq!(t.group_id, "plantA");
        assert_eq!(t.edge_node_id, "softplc-01");
        assert_eq!(t.device_id, "line");
        assert_eq!(t.ncmd(), "spBv1.0/plantA/NCMD/softplc-01");
        assert!(TopicIds::new("  ", "n", "d").is_err());
    }
}
