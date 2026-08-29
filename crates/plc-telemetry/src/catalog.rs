//! Device metric catalog and 1-based Sparkplug aliases.

use std::collections::BTreeSet;

use crate::error::TelemetryError;
use crate::types::MetricType;

/// One process tag published as a Sparkplug device metric.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogTag {
    /// Sparkplug metric name (e.g. `Conveyor1/RunFwd`).
    pub name: String,
    /// Value type.
    pub value_type: MetricType,
    /// `true` for `%I`; `false` for `%Q`.
    pub is_input: bool,
    /// Process-image slot (`TelemetrySample.tag_hint`).
    pub slot: u32,
    /// Engineering unit (`engUnit` property); empty omits the property.
    pub unit: String,
}

/// Catalog row with assigned alias.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    /// Stable Sparkplug alias (starts at 1).
    pub alias: u32,
    /// Tag definition.
    pub tag: CatalogTag,
}

/// Sorted tag list → Sparkplug aliases.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagCatalog {
    entries: Vec<CatalogEntry>,
}

impl TagCatalog {
    /// Assign aliases `1..=N` from names sorted lexicographically.
    pub fn from_tags(mut tags: Vec<CatalogTag>) -> Result<Self, TelemetryError> {
        tags.sort_by(|a, b| a.name.cmp(&b.name));
        let mut names = BTreeSet::new();
        let mut slots = BTreeSet::new();
        let mut entries = Vec::with_capacity(tags.len());
        for (i, tag) in tags.into_iter().enumerate() {
            if tag.name.trim().is_empty() {
                return Err(TelemetryError::config("catalog tag name must be non-empty"));
            }
            if !names.insert(tag.name.clone()) {
                return Err(TelemetryError::config(format!(
                    "duplicate metric name '{}'",
                    tag.name
                )));
            }
            if !slots.insert((tag.is_input, tag.slot)) {
                return Err(TelemetryError::config(format!(
                    "duplicate slot {}/{}",
                    if tag.is_input { "I" } else { "Q" },
                    tag.slot
                )));
            }
            entries.push(CatalogEntry {
                alias: u32::try_from(i)
                    .ok()
                    .and_then(|n| n.checked_add(1))
                    .ok_or_else(|| TelemetryError::config("too many catalog tags"))?,
                tag,
            });
        }
        Ok(Self { entries })
    }

    /// Fallback names `I{n}` / `Q{n}` (BOOL) for tests and pre-arm SIM.
    pub fn from_image_slots(n_inputs: usize, n_outputs: usize) -> Result<Self, TelemetryError> {
        let mut tags = Vec::with_capacity(n_inputs + n_outputs);
        for i in 0..n_inputs {
            tags.push(CatalogTag {
                name: format!("I{i}"),
                value_type: MetricType::Bool,
                is_input: true,
                slot: i as u32,
                unit: String::new(),
            });
        }
        for i in 0..n_outputs {
            tags.push(CatalogTag {
                name: format!("Q{i}"),
                value_type: MetricType::Bool,
                is_input: false,
                slot: i as u32,
                unit: String::new(),
            });
        }
        Self::from_tags(tags)
    }

    /// Look up by process-image slot.
    #[must_use]
    pub fn get(&self, is_input: bool, slot: u32) -> Option<&CatalogEntry> {
        self.entries
            .iter()
            .find(|e| e.tag.is_input == is_input && e.tag.slot == slot)
    }

    /// Ordered catalog (alias order = sorted-name order).
    #[must_use]
    pub fn entries(&self) -> &[CatalogEntry] {
        &self.entries
    }

    /// True when no device metrics are configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_from_sorted_names() {
        let cat = TagCatalog::from_tags(vec![
            CatalogTag {
                name: "Silo1/Level_eu".into(),
                value_type: MetricType::Real,
                is_input: true,
                slot: 1,
                unit: "pct".into(),
            },
            CatalogTag {
                name: "Conveyor1/RunFwd".into(),
                value_type: MetricType::Bool,
                is_input: false,
                slot: 0,
                unit: String::new(),
            },
        ])
        .unwrap();
        assert_eq!(cat.entries()[0].alias, 1);
        assert_eq!(cat.entries()[0].tag.name, "Conveyor1/RunFwd");
        assert_eq!(cat.entries()[1].alias, 2);
        assert_eq!(cat.entries()[1].tag.name, "Silo1/Level_eu");
        assert_eq!(cat.get(false, 0).unwrap().alias, 1);
        assert_eq!(cat.get(true, 1).unwrap().alias, 2);
    }

    #[test]
    fn duplicate_name_fails() {
        let tag = CatalogTag {
            name: "A".into(),
            value_type: MetricType::Bool,
            is_input: true,
            slot: 0,
            unit: String::new(),
        };
        let mut b = tag.clone();
        b.slot = 1;
        assert!(TagCatalog::from_tags(vec![tag, b]).is_err());
    }
}
