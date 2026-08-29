//! Shared API test helpers.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::extract::connect_info::ConnectInfo;
use axum::http::{header, Request, Response, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use plc_api::{router, AppState};
use plc_auth::{hash_secret, hex_encode};
use plc_config::{load_from_str, ConfigFormat, DeviceConfig};
use plc_io::ProcessImage;
use plc_io_sim::SimDriver;
use plc_ir::{assemble, IrType};
use plc_package::{IrTypeName, Manifest, PackageBuilder, RestartPolicy, TagEntry, TagKind};
use plc_runtime::{Runtime, RuntimeConfig};
use plc_scan::{MonotonicClock, ScanPlan};
use tower::ServiceExt;

pub const VIEWER: &str = "viewer-secret";
pub const OPERATOR: &str = "operator-secret";
pub const ENGINEER: &str = "engineer-secret";
pub const ENGINEER_B: &str = "engineer-b-secret";

const Q_WRITE: &str = r#"
.header data_size=8 retain_size=0 input_slots=1 output_slots=1
.entry task.main
PUSHI_BOOL 1
ST_Q       0
HALT
"#;

pub fn tmp_dir() -> PathBuf {
    let p = std::env::temp_dir().join(format!("plc-api-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&p).unwrap();
    p
}

pub fn hash_hex(secret: &str) -> String {
    hex_encode(&hash_secret(secret.as_bytes()))
}

pub fn cfg_yaml(programs: &str, required: bool, dual: bool) -> String {
    format!(
        r#"
version: 1
profile: dev
device:
  id: test-plc
scan:
  tasks:
    - name: main
      period_ms: 50
      entry: task.main
paths:
  programs: {programs}
  retain: {programs}/retain
  audit: {programs}/audit
auth:
  required: {required}
  dual_control: {dual}
  principals:
    - id: viewer
      role: viewer
      token_sha256: "{v}"
    - id: operator
      role: operator
      token_sha256: "{o}"
    - id: engineer
      role: engineer
      token_sha256: "{e}"
    - id: engineer-b
      role: engineer
      token_sha256: "{eb}"
rest:
  bind: "127.0.0.1:0"
"#,
        programs = programs,
        required = required,
        dual = dual,
        v = hash_hex(VIEWER),
        o = hash_hex(OPERATOR),
        e = hash_hex(ENGINEER),
        eb = hash_hex(ENGINEER_B),
    )
}

pub fn load_cfg(programs: &Path, required: bool, dual: bool) -> DeviceConfig {
    load_from_str(
        &cfg_yaml(&programs.to_string_lossy(), required, dual),
        ConfigFormat::Yaml,
    )
    .expect("test config")
}

pub fn make_state(required: bool, dual: bool) -> (AppState, PathBuf) {
    let root = tmp_dir();
    let cfg = load_cfg(&root, required, dual);
    let io = plc_scan::ScanIo::new(
        ProcessImage::with_sizes(1, 1, 0),
        Box::new(SimDriver::new("sim", 1, 1)),
    );
    let rt = Runtime::new(
        ScanPlan::from_config(&cfg).unwrap(),
        io,
        Box::new(MonotonicClock::new()),
        RuntimeConfig::default(),
    )
    .unwrap();
    let cfg_path = root.join("device.yaml");
    plc_config::save_to_path(&cfg_path, &cfg).unwrap();
    let state = AppState::new(cfg, rt, Some(cfg_path)).unwrap();
    (state, root)
}

pub fn app_open() -> Router {
    let (state, _) = make_state(false, false);
    router(state)
}

pub fn app_auth() -> (Router, AppState) {
    let (state, _) = make_state(true, false);
    let r = router(state.clone());
    (r, state)
}

pub fn app_dual() -> (Router, AppState) {
    let (state, _) = make_state(true, true);
    (router(state.clone()), state)
}

pub fn pack_line() -> Vec<u8> {
    pack("line", Q_WRITE, &[("Conveyor1/RunFwd", 0)])
}

pub fn pack(id: &str, spasm: &str, q_tags: &[(&str, u32)]) -> Vec<u8> {
    let module = assemble(spasm).expect("assemble");
    let mut task_entries = BTreeMap::new();
    task_entries.insert("main".into(), "task.main".into());
    let tag_dictionary = q_tags
        .iter()
        .map(|(name, slot)| TagEntry {
            name: (*name).into(),
            ty: IrTypeName(IrType::Bool),
            kind: TagKind::Q,
            slot: Some(*slot),
        })
        .collect();
    let manifest = Manifest {
        id: id.into(),
        version: "0.1.0".into(),
        build_id: "test".into(),
        ir_major: module.ir_major,
        ir_minor: module.ir_minor,
        primitive_abi: 1,
        task_entries,
        retain_symbols: Vec::new(),
        tag_dictionary,
        restart_policy: RestartPolicy::SafeReset,
        compatibility_hash: "00".repeat(32),
        input_slots: Some(module.input_slots),
        output_slots: Some(module.output_slots),
        data_size: Some(module.data_size),
        retain_size: Some(module.retain_size),
        const_size: Some(module.const_size),
    };
    PackageBuilder::new(manifest)
        .section_module(&module)
        .unwrap()
        .unsigned()
        .to_bytes()
        .unwrap()
}

pub fn bearer(token: &str) -> (header::HeaderName, header::HeaderValue) {
    (
        header::AUTHORIZATION,
        header::HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
    )
}

pub async fn send(app: Router, mut req: Request<Body>) -> (StatusCode, Vec<u8>) {
    req.extensions_mut().insert(ConnectInfo(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        1,
    )));
    let res: Response<Body> = app.oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, bytes.to_vec())
}

pub fn get(path: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(path)
        .body(Body::empty())
        .unwrap()
}

pub fn get_auth(path: &str, token: &str) -> Request<Body> {
    let (h, v) = bearer(token);
    Request::builder()
        .method("GET")
        .uri(path)
        .header(h, v)
        .body(Body::empty())
        .unwrap()
}

pub fn post_json(path: &str, token: Option<&str>, json: &str) -> Request<Body> {
    let mut b = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(t) = token {
        let (h, v) = bearer(t);
        b = b.header(h, v);
    }
    b.body(Body::from(json.to_string())).unwrap()
}

pub fn put_json(path: &str, token: Option<&str>, json: &str) -> Request<Body> {
    let mut b = Request::builder()
        .method("PUT")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(t) = token {
        let (h, v) = bearer(t);
        b = b.header(h, v);
    }
    b.body(Body::from(json.to_string())).unwrap()
}

pub fn post_bytes(path: &str, token: Option<&str>, bytes: Vec<u8>) -> Request<Body> {
    let mut b = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/octet-stream");
    if let Some(t) = token {
        let (h, v) = bearer(t);
        b = b.header(h, v);
    }
    b.body(Body::from(bytes)).unwrap()
}

pub fn delete_auth(path: &str, token: &str) -> Request<Body> {
    let (h, v) = bearer(token);
    Request::builder()
        .method("DELETE")
        .uri(path)
        .header(h, v)
        .body(Body::empty())
        .unwrap()
}
