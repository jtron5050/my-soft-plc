//! Liveness and OpenAPI presence.

mod common;

use axum::http::StatusCode;

use common::{app_open, get, send};

#[tokio::test]
async fn health_unauthenticated() {
    let (status, body) = send(app_open(), get("/api/v1/health")).await;
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["status"], "ok");
}

#[tokio::test]
async fn health_ok_when_auth_required() {
    let (app, _) = common::app_auth();
    let (status, _) = send(app, get("/api/v1/health")).await;
    assert_eq!(status, StatusCode::OK);
}

#[test]
fn openapi_lists_resources() {
    let text = include_str!("../../../docs/openapi/openapi.yaml");
    for path in [
        "/api/v1/health",
        "/api/v1/status",
        "/api/v1/programs",
        "/api/v1/mode",
        "/api/v1/tags",
        "/api/v1/metrics",
        "/api/v1/diagnostics/events",
        "/api/v1/diagnostics/audit",
    ] {
        assert!(text.contains(path), "missing {path}");
    }
}
