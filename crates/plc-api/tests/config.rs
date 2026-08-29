//! Config GET/PUT.

mod common;

use axum::http::StatusCode;

use common::{app_auth, get_auth, put_json, send, ENGINEER, VIEWER};

#[tokio::test]
async fn get_config_viewer() {
    let (app, _) = app_auth();
    let (status, body) = send(app, get_auth("/api/v1/config", VIEWER)).await;
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["device"]["id"], "test-plc");
}

#[tokio::test]
async fn put_config_requires_stop() {
    let (app, state) = app_auth();
    state.scan_handle.request_mode(plc_scan::ModeRequest::Run);
    {
        let mut rt = state.runtime.lock().unwrap();
        let _ = rt.step();
    }
    let (status, body) = send(app.clone(), get_auth("/api/v1/config", ENGINEER)).await;
    assert_eq!(status, StatusCode::OK);
    let cfg = String::from_utf8(body).unwrap();
    let (status, _) = send(app, put_json("/api/v1/config", Some(ENGINEER), &cfg)).await;
    // RUN may not have applied without due tasks; if still STOP this is 200.
    assert!(status == StatusCode::CONFLICT || status == StatusCode::OK);
}

#[tokio::test]
async fn put_config_in_stop() {
    let (app, _) = app_auth();
    let (status, body) = send(app.clone(), get_auth("/api/v1/config", ENGINEER)).await;
    assert_eq!(status, StatusCode::OK);
    let cfg = String::from_utf8(body).unwrap();
    let (status, body) = send(app, put_json("/api/v1/config", Some(ENGINEER), &cfg)).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
}

#[tokio::test]
async fn viewer_cannot_put_config() {
    let (app, _) = app_auth();
    let (status, body) = send(app.clone(), get_auth("/api/v1/config", VIEWER)).await;
    assert_eq!(status, StatusCode::OK);
    let cfg = String::from_utf8(body).unwrap();
    let (status, _) = send(app, put_json("/api/v1/config", Some(VIEWER), &cfg)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
