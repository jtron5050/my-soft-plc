//! Aliases from sorted tag names, starting at 1.

use plc_telemetry::{CatalogTag, MetricType, TagCatalog};

#[test]
fn conveyor_before_silo() {
    let cat = TagCatalog::from_tags(vec![
        CatalogTag {
            name: "Silo1/Level_eu".into(),
            value_type: MetricType::Real,
            is_input: true,
            slot: 0,
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
    assert_eq!(cat.get(false, 0).unwrap().alias, 1);
    assert_eq!(cat.get(false, 0).unwrap().tag.name, "Conveyor1/RunFwd");
    assert_eq!(cat.get(true, 0).unwrap().alias, 2);
}
