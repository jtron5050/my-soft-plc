//! Topic layout vs Sparkplug node/device split.

use plc_telemetry::TopicIds;

#[test]
fn n_verbs_have_four_tokens() {
    let ids = TopicIds::new("plantA", "softplc-01", "line").unwrap();
    for t in [ids.nbirth(), ids.ndata(), ids.ndeath(), ids.ncmd()] {
        assert_eq!(t.split('/').count(), 4, "{t}");
        assert!(!t.ends_with("/line"), "{t}");
    }
}

#[test]
fn d_verbs_have_five_tokens() {
    let ids = TopicIds::new("plantA", "softplc-01", "line").unwrap();
    for t in [ids.dbirth(), ids.ddata(), ids.ddeath()] {
        assert_eq!(t.split('/').count(), 5, "{t}");
        assert!(t.ends_with("/line"), "{t}");
    }
}

#[test]
fn plus_and_hash_are_rejected_and_ids_are_trimmed() {
    assert!(TopicIds::new("g+", "e", "d").is_err());
    assert!(TopicIds::new("g", "e#", "d").is_err());
    assert!(TopicIds::new("#", "e", "d").is_err());
    let ids = TopicIds::new(" plantA ", " softplc-01 ", " line ").unwrap();
    assert_eq!(ids.ncmd(), "spBv1.0/plantA/NCMD/softplc-01");
}
