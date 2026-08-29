//! Config GET/PUT.

mod common;

use axum::http::StatusCode;

use common::{app_auth, bring_up_run, get_auth, put_json, send, ENGINEER, VIEWER};

#[tokio::test]
async fn get_config_viewer() {
    let (app, _) = app_auth();
    let (status, body) = send(app, get_auth("/api/v1/config", VIEWER)).await;
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["device"]["id"], "test-plc");
    let principals = v["auth"]["principals"].as_array().unwrap();
    assert!(!principals.is_empty());
    for p in principals {
        assert!(p.get("token_sha256").is_none());
        assert!(p.get("cert_sha256").is_none());
    }
    assert_eq!(v["auth"]["tls_key_path"], "");
}

#[tokio::test]
async fn put_config_requires_stop() {
    let (app, state) = app_auth();
    bring_up_run(app.clone(), &state).await;
    let cfg = serde_json::to_string(&*state.config.read().unwrap()).unwrap();
    let (status, _) = send(app, put_json("/api/v1/config", Some(ENGINEER), &cfg)).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn put_config_in_stop() {
    let (app, state) = app_auth();
    let cfg = serde_json::to_string(&*state.config.read().unwrap()).unwrap();
    let (status, body) = send(app, put_json("/api/v1/config", Some(ENGINEER), &cfg)).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["restart_required"], false);
}

#[tokio::test]
async fn put_config_bind_sets_restart_required() {
    let (app, state) = app_auth();
    let mut cfg = state.config.read().unwrap().clone();
    cfg.rest.bind = "0.0.0.0:1".into();
    let body = serde_json::to_string(&cfg).unwrap();
    let (status, body) = send(app, put_json("/api/v1/config", Some(ENGINEER), &body)).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["restart_required"], true);
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
