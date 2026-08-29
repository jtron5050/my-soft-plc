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
        let ids = Self {
            group_id: group_id.into(),
            edge_node_id: edge_node_id.into(),
            device_id: device_id.into(),
        };
        if ids.group_id.trim().is_empty() {
            return Err(TelemetryError::config("group_id must be non-empty"));
        }
        if ids.edge_node_id.trim().is_empty() {
            return Err(TelemetryError::config("edge_node_id must be non-empty"));
        }
        if ids.device_id.trim().is_empty() {
            return Err(TelemetryError::config("device_id must be non-empty"));
        }
        for (label, value) in [
            ("group_id", ids.group_id.as_str()),
            ("edge_node_id", ids.edge_node_id.as_str()),
            ("device_id", ids.device_id.as_str()),
        ] {
            if value.contains('/') {
                return Err(TelemetryError::config(format!(
                    "{label} must not contain '/'"
                )));
            }
        }
        Ok(ids)
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

    /// True when `topic` is this node's NCMD topic.
    #[must_use]
    pub fn is_ncmd(&self, topic: &str) -> bool {
        topic == self.ncmd()
    }
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
    }

    #[test]
    fn slash_in_id_is_rejected() {
        assert!(TopicIds::new("a/b", "n", "d").is_err());
    }
}
