//! Dual-control helper.

use std::sync::Arc;

use plc_auth::{dual_control_allowed, AuthService, FakeClock, ANONYMOUS_ID};
use plc_config::AuthConfig;

use common::{limits, required_auth};

mod common;

#[test]
fn disabled_always_allows() {
    assert!(dual_control_allowed(false, "a", "a"));
    assert!(dual_control_allowed(false, "", ""));
    assert!(dual_control_allowed(false, ANONYMOUS_ID, ANONYMOUS_ID));
}

#[test]
fn enabled_requires_distinct_named_principals() {
    assert!(dual_control_allowed(true, "eng-a", "eng-b"));
    assert!(!dual_control_allowed(true, "eng-a", "eng-a"));
    assert!(!dual_control_allowed(true, "", "eng-b"));
    assert!(!dual_control_allowed(true, "eng-a", ""));
    assert!(!dual_control_allowed(true, ANONYMOUS_ID, "eng-b"));
    assert!(!dual_control_allowed(true, "eng-a", ANONYMOUS_ID));
}

#[test]
fn service_flag() {
    let clock = Arc::new(FakeClock::new());
    let off_cfg = AuthConfig {
        dual_control: false,
        ..required_auth()
    };
    let off = AuthService::from_config_with_clock(&off_cfg, &limits(), clock.clone()).unwrap();
    assert!(off.dual_control_ok("a", "a"));

    let on_cfg = AuthConfig {
        dual_control: true,
        ..required_auth()
    };
    let on = AuthService::from_config_with_clock(&on_cfg, &limits(), clock).unwrap();
    assert!(!on.dual_control_ok("a", "a"));
    assert!(on.dual_control_ok("a", "b"));
}

#[test]
fn default_config_dual_control_off() {
    let svc = AuthService::from_config(&AuthConfig::default(), &limits()).unwrap();
    assert!(svc.dual_control_ok("same", "same"));
}
