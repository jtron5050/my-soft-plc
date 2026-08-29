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
    assert_eq!(ids.ddata().split('/').count(), 5);
    assert!(ids.ddata().ends_with("/line"));
}
