//! Field-driver gate on boot: Modbus TCP and GPIO are accepted when the map matches.

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
async fn gpio_without_a_module_is_refused() {
    let cfg = cfg_with(&["sim", "gpio"], "samples/configs/sim-plant-io-map.yaml");
    let err = match Supervisor::boot(
        cfg,
        Some(repo_root().join("samples/configs/sim-plant.yaml")),
        None,
        None,
    )
    .await
    {
        Ok(_) => panic!("gpio with a sim-only map should be refused"),
        Err(err) => err,
    };
    let text = err.to_string();
    assert!(text.contains("no gpio module"), "{text}");
    assert!(!text.contains("PR-17"), "{text}");
}

#[tokio::test]
async fn gpio_missing_chip_fails_at_open() {
    let map_path =
        std::env::temp_dir().join(format!("soft-plc-gpio-map-{}.yaml", std::process::id()));
    std::fs::write(
        &map_path,
        r#"
version: 1
modules:
  - id: local_di_1
    driver: gpio
    config: { chip: gpiochip99, lines: [0] }
    bindings:
      - tag: Pull
        image: I
        bit: 0
"#,
    )
    .unwrap();
    let cfg = cfg_with(&["gpio"], map_path.to_str().expect("utf8 path"));
    let err = match Supervisor::boot(
        cfg,
        Some(repo_root().join("samples/configs/sim-plant.yaml")),
        None,
        None,
    )
    .await
    {
        Ok(_) => panic!("gpiochip99 should not open"),
        Err(err) => err,
    };
    let text = err.to_string();
    assert!(text.contains("gpiochip99"), "{text}");
    assert!(!text.contains("PR-17"), "{text}");
    let _ = std::fs::remove_file(map_path);
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
