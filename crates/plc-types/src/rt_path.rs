//! RT-path discipline scaffolding (KD-13).
//!
//! Future crates on the scan thread (`plc-scan`, `plc-vm`, and native FB
//! primitives used from the VM) must not depend on tokio, network crates, or
//! blocking filesystem APIs. Enforcement layers:
//!
//! 1. **Crate dependencies** — RT crates list only RT-safe deps (prefer none
//!    beyond `plc-types` and small `no_std`-friendly helpers).
//! 2. **Workspace / CI clippy** — deny accidental use as crates appear.
//! 3. **`deny.toml` [bans]** — optional cargo-deny rules when RT crates land.
//!
//! This module documents the intended RT crate set and forbidden dependency
//! names so CI and humans share one source of truth before those crates exist.

/// Crate names expected to run (or be callable from) the RT scan path.
pub const RT_PATH_CRATES: &[&str] = &[
    "plc-scan",
    "plc-vm",
    "plc-fb-primitives",
    // `plc-types` is shared; it must stay free of forbidden deps (enforced here).
    "plc-types",
];

/// Dependency crate names that must never appear in RT-path crate graphs.
///
/// Not exhaustive — network / OS I/O families grow; treat as a starting ban list.
pub const RT_FORBIDDEN_CRATE_HINTS: &[&str] = &[
    "tokio",
    "tokio-util",
    "hyper",
    "hyper-util",
    "reqwest",
    "axum",
    "warp",
    "actix-web",
    "mio",
    "socket2",
    "rustls",
    "native-tls",
    "openssl",
    "rumqttc",
    "paho-mqtt",
    // Filesystem / blocking I/O helpers that belong on non-RT workers only.
    "tokio-fs",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rt_crate_list_includes_foundation() {
        assert!(RT_PATH_CRATES.contains(&"plc-types"));
        assert!(RT_PATH_CRATES.contains(&"plc-scan"));
        assert!(RT_PATH_CRATES.contains(&"plc-vm"));
    }

    #[test]
    fn forbidden_list_includes_tokio() {
        assert!(RT_FORBIDDEN_CRATE_HINTS.contains(&"tokio"));
    }
}
