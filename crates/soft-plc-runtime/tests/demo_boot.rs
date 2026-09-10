//! In-process boot of the SIM demo (no MQTT broker).

use std::path::{Path, PathBuf};
use std::time::Duration;

use plc_config::{load_from_path, DeviceConfig, ProfileKind};
use plc_retain::RetainStore;
use soft_plc_runtime::{apply_cli, Supervisor};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn demo_cfg(tmp: &Path) -> DeviceConfig {
    let root = repo_root();
    let mut cfg = load_from_path(&root.join("samples/configs/sim-plant.yaml")).unwrap();
    assert_eq!(cfg.profile, ProfileKind::Dev);
    cfg.telemetry.enabled = false;
    apply_cli(&mut cfg, Some(&tmp.to_path_buf()), Some("127.0.0.1:0"));
    cfg
}

fn demo_program() -> PathBuf {
    repo_root().join("samples/programs/demo-conveyor/fixture.spkg")
}

fn demo_config_path() -> PathBuf {
    repo_root().join("samples/configs/sim-plant.yaml")
}

async fn http_get(addr: std::net::SocketAddr, path: &str) -> (u16, String) {
    let mut s = TcpStream::connect(addr).await.expect("connect");
    let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf);
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (status, text.into_owned())
}

#[tokio::test]
async fn boot_health_and_demo_start() {
    let root = repo_root();
    let tmp = std::env::temp_dir().join(format!("soft-plc-demo-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let mut cfg = load_from_path(&root.join("samples/configs/sim-plant.yaml")).unwrap();
    assert_eq!(cfg.profile, ProfileKind::Dev);
    cfg.telemetry.enabled = false;
    apply_cli(&mut cfg, Some(&tmp), Some("127.0.0.1:0"));
    let program = root.join("samples/programs/demo-conveyor/fixture.spkg");
    let sup = Supervisor::boot(
        cfg,
        Some(root.join("samples/configs/sim-plant.yaml")),
        Some(program),
        Some("SIM"),
    )
    .await
    .expect("boot");
    let addr = sup.listen;
    let serve = tokio::spawn(async move { sup.serve().await });
    tokio::time::sleep(Duration::from_millis(50)).await;

    let (st, body) = http_get(addr, "/api/v1/health").await;
    assert_eq!(st, 200, "{body}");
    assert!(body.contains("ok"), "{body}");

    let (st, body) = http_get(addr, "/api/v1/status").await;
    assert_eq!(st, 200, "{body}");
    assert!(
        body.contains("demo-conveyor") || body.contains("SIM"),
        "{body}"
    );

    serve.abort();
}

#[test]
fn cli_help() {
    let bin = env!("CARGO_BIN_EXE_soft-plc-runtime");
    let out = std::process::Command::new(bin)
        .arg("--help")
        .output()
        .expect("help");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("--config"));
    assert!(stdout.contains("--program"));
}

#[tokio::test]
async fn boot_program_writes_current_pointer_and_restore_reloads_it() {
    let tmp = std::env::temp_dir().join(format!("soft-plc-ptr-{}", uuid_stamp()));
    std::fs::create_dir_all(&tmp).unwrap();
    let cfg = demo_cfg(&tmp);
    let sup = Supervisor::boot(cfg, Some(demo_config_path()), Some(demo_program()), None)
        .await
        .expect("boot");
    assert_eq!(
        sup.state.store.pointer("current").as_deref(),
        Some("demo-conveyor")
    );
    assert!(sup.state.store.pointer("armed").is_none());
    drop(sup);

    let cfg = demo_cfg(&tmp);
    let sup = Supervisor::boot(cfg, Some(demo_config_path()), None, None)
        .await
        .expect("restore");
    {
        let rt = sup.state.runtime.lock().unwrap();
        assert_eq!(
            rt.current_info().map(|p| p.id.as_str()),
            Some("demo-conveyor")
        );
    }
    drop(sup);
}

#[tokio::test]
async fn boot_program_restores_retain() {
    let tmp = std::env::temp_dir().join(format!("soft-plc-ret-{}", uuid_stamp()));
    std::fs::create_dir_all(&tmp).unwrap();
    let cfg = demo_cfg(&tmp);
    let retain_dir = cfg.paths.retain.clone();
    let sup = Supervisor::boot(cfg, Some(demo_config_path()), Some(demo_program()), None)
        .await
        .expect("boot");
    let (id, layout) = {
        let rt = sup.state.runtime.lock().unwrap();
        (
            rt.current_info().unwrap().id.clone(),
            rt.current_retain_layout().unwrap().clone(),
        )
    };
    drop(sup);

    let planted = 42.0f32.to_le_bytes();
    let mut image = vec![0u8; layout.retain_size as usize];
    image[..planted.len()].copy_from_slice(&planted);
    RetainStore::open(&retain_dir)
        .unwrap()
        .flush(&id, &layout, &image)
        .unwrap();

    let cfg = demo_cfg(&tmp);
    let sup = Supervisor::boot(cfg, Some(demo_config_path()), Some(demo_program()), None)
        .await
        .expect("boot with --program");
    {
        let rt = sup.state.runtime.lock().unwrap();
        let got = rt.current_retain_bytes().expect("retain");
        assert_eq!(&got[..planted.len()], &planted);
    }
    drop(sup);
}

#[tokio::test]
async fn boot_bind_failure_does_not_start() {
    let tmp = std::env::temp_dir().join(format!("soft-plc-bind-{}", uuid_stamp()));
    std::fs::create_dir_all(&tmp).unwrap();
    let mut cfg = demo_cfg(&tmp);
    // TEST-NET-1: not assigned on this host, so bind fails after AppState is built.
    cfg.rest.bind = "192.0.2.1:9".into();
    let err = match Supervisor::boot(cfg, Some(demo_config_path()), None, None).await {
        Ok(_) => panic!("bind must fail"),
        Err(e) => e,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("192.0.2.1") || msg.contains("Cannot assign") || msg.contains("os error"),
        "{msg}"
    );
}

fn uuid_stamp() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}
