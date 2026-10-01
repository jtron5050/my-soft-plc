//! Field-driver gate on boot: Modbus TCP is accepted, GPIO is not.

use std::path::PathBuf;

use plc_config::load_from_path;
use soft_plc_runtime::{apply_cli, Supervisor};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn cfg_with(drivers: &[&str], io_map: &str) -> plc_config::DeviceConfig {
    let root = repo_root();
    let mut cfg = load_from_path(&root.join("samples/configs/sim-plant.yaml")).unwrap();
    cfg.telemetry.enabled = false;
    cfg.io.drivers = drivers.iter().map(|d| (*d).to_string()).collect();
    cfg.paths.io_map = io_map.to_string();
    let tmp = std::env::temp_dir().join(format!(
        "soft-plc-drv-{}-{}",
        std::process::id(),
        drivers.join("-")
    ));
    std::fs::create_dir_all(&tmp).unwrap();
    apply_cli(&mut cfg, Some(&tmp), Some("127.0.0.1:0"));
    cfg
}

#[tokio::test]
async fn gpio_is_still_refused() {
    let cfg = cfg_with(&["gpio"], "samples/configs/sim-plant-io-map.yaml");
    let err = match Supervisor::boot(
        cfg,
        Some(repo_root().join("samples/configs/sim-plant.yaml")),
        None,
        None,
    )
    .await
    {
        Ok(_) => panic!("gpio should be refused"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("PR-17"), "{err}");
}

#[tokio::test]
async fn modbus_without_a_module_is_refused() {
    let cfg = cfg_with(&["modbus_tcp"], "samples/configs/sim-plant-io-map.yaml");
    let err = match Supervisor::boot(
        cfg,
        Some(repo_root().join("samples/configs/sim-plant.yaml")),
        None,
        None,
    )
    .await
    {
        Ok(_) => panic!("modbus_tcp with a sim-only map should be refused"),
        Err(err) => err,
    };
    let text = err.to_string();
    assert!(
        text.contains("not listed") || text.contains("no modbus_tcp module"),
        "{text}"
    );
}

#[tokio::test]
async fn modbus_tcp_boots() {
    let cfg = cfg_with(&["modbus_tcp"], "samples/configs/modbus-rack-io-map.yaml");
    let mut sup = Supervisor::boot(
        cfg,
        Some(repo_root().join("samples/configs/sim-plant.yaml")),
        None,
        None,
    )
    .await
    .expect("modbus boot");
    sup.shutdown();
}
