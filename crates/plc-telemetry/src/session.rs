//! Sparkplug session: `bdSeq`, wrap-around `seq`, birth/death payloads.

use std::collections::BTreeMap;

use plc_io::PlcValue;
use plc_scan::TelemetrySample;
use plc_types::{OperatingMode, Quality};

use crate::catalog::TagCatalog;
use crate::protobuf::{Metric, MetricValue, Payload, Property};
use crate::types::{
    publish_quality, quality_code, value_to_sparkplug, MetricType, SP_BOOLEAN, SP_INT32, SP_INT64,
    SP_STRING, SP_UINT64,
};

/// Last DDATA sample used to stamp DBIRTH.
#[derive(Debug, Clone, Copy)]
struct CachedDeviceValue {
    value: PlcValue,
    quality: Quality,
    forced: bool,
}

/// `bdSeq` metric name.
pub const METRIC_BDSEQ: &str = "bdSeq";
/// Host rebirth command / node control metric.
pub const METRIC_REBIRTH: &str = "Node Control/Rebirth";
/// Operator mode.
pub const METRIC_MODE: &str = "SYSTEM/Mode";
/// Scan-side SPSC drop counter.
pub const METRIC_DROPS: &str = "telemetry_drops";

/// Sparkplug sequence and birth/death encoder (no MQTT).
#[derive(Debug, Clone)]
pub struct SessionState {
    bd_seq: u64,
    node_seq: u8,
    device_seq: u8,
    first_session: bool,
    catalog: TagCatalog,
    last_mode: Option<OperatingMode>,
    last_drops: Option<u64>,
    last_device: BTreeMap<(bool, u32), CachedDeviceValue>,
}

impl Default for SessionState {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionState {
    /// First MQTT session will use `bdSeq = 0`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            bd_seq: 0,
            node_seq: 0,
            device_seq: 0,
            first_session: true,
            catalog: TagCatalog::default(),
            last_mode: None,
            last_drops: None,
            last_device: BTreeMap::new(),
        }
    }

    /// Current `bdSeq` (matches the Will / next NBIRTH).
    #[must_use]
    pub fn bd_seq(&self) -> u64 {
        self.bd_seq
    }

    /// Replace the device metric catalog (arm / hot-swap).
    pub fn set_catalog(&mut self, catalog: TagCatalog) {
        self.catalog = catalog;
        self.last_device
            .retain(|&(is_input, slot), _| self.catalog.get(is_input, slot).is_some());
    }

    /// Catalog currently armed.
    #[must_use]
    pub fn catalog(&self) -> &TagCatalog {
        &self.catalog
    }

    /// New MQTT session: increment `bdSeq` after the first connect; reset seq.
    pub fn prepare_connect(&mut self) {
        if !self.first_session {
            self.bd_seq = self.bd_seq.wrapping_add(1);
        }
        self.first_session = false;
        self.node_seq = 0;
        self.device_seq = 0;
        self.last_mode = None;
        self.last_drops = None;
    }

    /// Host rebirth on the same MQTT session: seq back to 0, `bdSeq` unchanged.
    pub fn on_rebirth(&mut self) {
        self.node_seq = 0;
        self.device_seq = 0;
        self.last_mode = None;
        self.last_drops = None;
    }

    /// NDEATH Will payload (`bdSeq` only).
    #[must_use]
    pub fn ndeath(&self, timestamp_ms: u64) -> Payload {
        Payload {
            timestamp: Some(timestamp_ms),
            seq: Some(0),
            metrics: vec![bdseq_metric(self.bd_seq, timestamp_ms)],
        }
    }

    /// NBIRTH (`seq = 0`) with node metrics.
    pub fn nbirth(
        &mut self,
        timestamp_ms: u64,
        mode: OperatingMode,
        drops: u64,
        quality: Quality,
    ) -> Payload {
        self.node_seq = 0;
        self.last_mode = Some(mode);
        self.last_drops = Some(drops);
        Payload {
            timestamp: Some(timestamp_ms),
            seq: Some(0),
            metrics: vec![
                bdseq_metric(self.bd_seq, timestamp_ms),
                named_bool(METRIC_REBIRTH, false, timestamp_ms, quality),
                named_string(METRIC_MODE, mode.as_str(), timestamp_ms, quality),
                named_int64(METRIC_DROPS, drops as i64, timestamp_ms, quality),
            ],
        }
    }

    /// DBIRTH (`seq = 0`) full device catalog. Empty catalog → `None`.
    ///
    /// Uses last-seen live `(value, quality, forced)` per slot; type defaults
    /// only before any sample for that slot.
    pub fn dbirth(&mut self, timestamp_ms: u64, clock_synced: bool) -> Option<Payload> {
        if self.catalog.is_empty() {
            return None;
        }
        self.device_seq = 0;
        let mut metrics = Vec::with_capacity(self.catalog.entries().len());
        for entry in self.catalog.entries() {
            let cached = self
                .last_device
                .get(&(entry.tag.is_input, entry.tag.slot))
                .and_then(|c| cached_metric(c, entry.tag.value_type, clock_synced));
            let (value, quality, forced) = cached.unwrap_or_else(|| {
                (
                    entry.tag.value_type.default_value(),
                    publish_quality(Quality::Good, clock_synced),
                    false,
                )
            });
            let mut m = Metric::new(entry.tag.value_type.sparkplug_datatype());
            m.name = Some(entry.tag.name.clone());
            m.alias = Some(u64::from(entry.alias));
            m.timestamp = Some(timestamp_ms);
            m.value = Some(value);
            m.properties = metric_properties(quality, forced, &entry.tag.unit);
            metrics.push(m);
        }
        Some(Payload {
            timestamp: Some(timestamp_ms),
            seq: Some(0),
            metrics,
        })
    }

    /// DDEATH (`seq` next after last DDATA) for catalog replace / device offline.
    #[must_use]
    pub fn ddeath(&mut self, timestamp_ms: u64) -> Payload {
        Payload {
            timestamp: Some(timestamp_ms),
            seq: Some(u64::from(next_seq(&mut self.device_seq))),
            metrics: Vec::new(),
        }
    }

    /// NDATA when mode or drop count changed. `None` if nothing new.
    pub fn ndata(
        &mut self,
        timestamp_ms: u64,
        mode: OperatingMode,
        drops: u64,
        quality: Quality,
    ) -> Option<Payload> {
        let mut metrics = Vec::new();
        if self.last_mode != Some(mode) {
            metrics.push(named_string(
                METRIC_MODE,
                mode.as_str(),
                timestamp_ms,
                quality,
            ));
            self.last_mode = Some(mode);
        }
        if self.last_drops != Some(drops) {
            metrics.push(named_int64(
                METRIC_DROPS,
                drops as i64,
                timestamp_ms,
                quality,
            ));
            self.last_drops = Some(drops);
        }
        if metrics.is_empty() {
            return None;
        }
        Some(Payload {
            timestamp: Some(timestamp_ms),
            seq: Some(u64::from(next_seq(&mut self.node_seq))),
            metrics,
        })
    }

    /// DDATA for process samples (alias only). Unknown slots dropped.
    pub fn ddata(
        &mut self,
        timestamp_ms: u64,
        samples: &[TelemetrySample],
        clock_synced: bool,
    ) -> Option<Payload> {
        let mut metrics = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        // Last sample for a slot wins.
        for sample in samples.iter().rev() {
            if !seen.insert((sample.is_input, sample.tag_hint)) {
                continue;
            }
            let Some(entry) = self.catalog.get(sample.is_input, sample.tag_hint) else {
                continue;
            };
            self.last_device.insert(
                (sample.is_input, sample.tag_hint),
                CachedDeviceValue {
                    value: sample.value,
                    quality: sample.quality,
                    forced: sample.forced,
                },
            );
            let q = publish_quality(sample.quality, clock_synced);
            let (datatype, value) = value_to_sparkplug(sample.value);
            let mut m = Metric::new(datatype);
            m.alias = Some(u64::from(entry.alias));
            m.timestamp = Some(timestamp_ms);
            m.value = Some(value);
            m.properties = metric_properties(q, sample.forced, "");
            metrics.push(m);
        }
        metrics.reverse();
        if metrics.is_empty() {
            return None;
        }
        Some(Payload {
            timestamp: Some(timestamp_ms),
            seq: Some(u64::from(next_seq(&mut self.device_seq))),
            metrics,
        })
    }
}

fn next_seq(seq: &mut u8) -> u8 {
    *seq = seq.wrapping_add(1);
    *seq
}

fn plc_matches_metric(value: PlcValue, ty: MetricType) -> bool {
    matches!(
        (value, ty),
        (PlcValue::Bool(_), MetricType::Bool)
            | (PlcValue::Int(_), MetricType::Int)
            | (PlcValue::Dint(_), MetricType::Dint)
            | (PlcValue::Real(_), MetricType::Real)
            | (PlcValue::Time(_), MetricType::Time)
    )
}

fn cached_metric(
    cached: &CachedDeviceValue,
    ty: MetricType,
    clock_synced: bool,
) -> Option<(MetricValue, Quality, bool)> {
    if !plc_matches_metric(cached.value, ty) {
        return None;
    }
    let (_, value) = value_to_sparkplug(cached.value);
    Some((
        value,
        publish_quality(cached.quality, clock_synced),
        cached.forced,
    ))
}

fn metric_properties(quality: Quality, forced: bool, unit: &str) -> Vec<Property> {
    let mut props = vec![Property {
        key: "Quality".into(),
        datatype: SP_INT32,
        value: MetricValue::Int(quality_code(quality) as u32),
    }];
    if forced {
        props.push(Property {
            key: "Forced".into(),
            datatype: SP_BOOLEAN,
            value: MetricValue::Bool(true),
        });
    }
    if !unit.is_empty() {
        props.push(Property {
            key: "engUnit".into(),
            datatype: SP_STRING,
            value: MetricValue::String(unit.to_string()),
        });
    }
    props
}

fn bdseq_metric(bd_seq: u64, ts: u64) -> Metric {
    let mut m = Metric::new(SP_UINT64);
    m.name = Some(METRIC_BDSEQ.into());
    m.timestamp = Some(ts);
    m.value = Some(MetricValue::Long(bd_seq));
    m
}

fn named_bool(name: &str, value: bool, ts: u64, quality: Quality) -> Metric {
    let mut m = Metric::new(SP_BOOLEAN);
    m.name = Some(name.into());
    m.timestamp = Some(ts);
    m.value = Some(MetricValue::Bool(value));
    m.properties = metric_properties(quality, false, "");
    m
}

fn named_string(name: &str, value: &str, ts: u64, quality: Quality) -> Metric {
    let mut m = Metric::new(SP_STRING);
    m.name = Some(name.into());
    m.timestamp = Some(ts);
    m.value = Some(MetricValue::String(value.into()));
    m.properties = metric_properties(quality, false, "");
    m
}

fn named_int64(name: &str, value: i64, ts: u64, quality: Quality) -> Metric {
    let mut m = Metric::new(SP_INT64);
    m.name = Some(name.into());
    m.timestamp = Some(ts);
    m.value = Some(MetricValue::Long(value as u64));
    m.properties = metric_properties(quality, false, "");
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{CatalogTag, TagCatalog};
    use crate::types::MetricType;
    use plc_io::PlcValue;

    #[test]
    fn first_connect_bdseq_zero_then_increments() {
        let mut s = SessionState::new();
        s.prepare_connect();
        assert_eq!(s.bd_seq(), 0);
        let death = s.ndeath(10);
        assert_eq!(death.metrics[0].value, Some(MetricValue::Long(0)));
        s.prepare_connect();
        assert_eq!(s.bd_seq(), 1);
    }

    #[test]
    fn nbirth_seq_zero_and_device_seq_wraps() {
        let mut s = SessionState::new();
        s.prepare_connect();
        s.set_catalog(
            TagCatalog::from_tags(vec![CatalogTag {
                name: "A".into(),
                value_type: MetricType::Bool,
                is_input: true,
                slot: 0,
                unit: String::new(),
            }])
            .unwrap(),
        );
        let birth = s.nbirth(1, OperatingMode::Stop, 0, Quality::Good);
        assert_eq!(birth.seq, Some(0));
        assert!(birth
            .metrics
            .iter()
            .any(|m| m.name.as_deref() == Some(METRIC_BDSEQ)));
        let db = s.dbirth(1, true).unwrap();
        assert_eq!(db.seq, Some(0));
        assert_eq!(db.metrics[0].alias, Some(1));
        assert!(db.metrics[0].name.is_some());
        assert_eq!(db.metrics[0].value, Some(MetricValue::Bool(false)));

        let sample = TelemetrySample {
            alias: 0,
            tag_hint: 0,
            value: PlcValue::Bool(true),
            quality: Quality::Good,
            forced: false,
            now_ms: 0,
            is_input: true,
        };
        for expected in 1_u64..=255 {
            let p = s.ddata(2, &[sample], true).unwrap();
            assert_eq!(p.seq, Some(expected));
            assert!(p.metrics[0].name.is_none());
            assert_eq!(p.metrics[0].alias, Some(1));
        }
        let wrap = s.ddata(3, &[sample], true).unwrap();
        assert_eq!(wrap.seq, Some(0));
    }

    #[test]
    fn rebirth_keeps_bdseq() {
        let mut s = SessionState::new();
        s.prepare_connect();
        s.prepare_connect();
        assert_eq!(s.bd_seq(), 1);
        s.on_rebirth();
        assert_eq!(s.bd_seq(), 1);
        let b = s.nbirth(5, OperatingMode::Run, 3, Quality::Good);
        assert_eq!(b.seq, Some(0));
    }

    #[test]
    fn dbirth_uses_cached_live_value_and_forced() {
        let mut s = SessionState::new();
        s.set_catalog(
            TagCatalog::from_tags(vec![CatalogTag {
                name: "A".into(),
                value_type: MetricType::Bool,
                is_input: true,
                slot: 0,
                unit: String::new(),
            }])
            .unwrap(),
        );
        let sample = TelemetrySample {
            alias: 0,
            tag_hint: 0,
            value: PlcValue::Bool(true),
            quality: Quality::Good,
            forced: true,
            now_ms: 0,
            is_input: true,
        };
        assert!(s.ddata(2, &[sample], true).is_some());
        let db = s.dbirth(3, true).unwrap();
        assert_eq!(db.metrics[0].value, Some(MetricValue::Bool(true)));
        let forced = db.metrics[0]
            .properties
            .iter()
            .find(|p| p.key == "Forced")
            .expect("Forced on birth when overlay is active");
        assert_eq!(forced.value, MetricValue::Bool(true));
    }
}
