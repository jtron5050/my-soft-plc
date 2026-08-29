//! Status and mode transitions.

mod common;

use axum::http::StatusCode;

use common::{app_auth, bring_up_run, get_auth, post_json, send, OPERATOR, VIEWER};

#[tokio::test]
async fn status_shape() {
    let (app, _) = app_auth();
    let (status, body) = send(app, get_auth("/api/v1/status", VIEWER)).await;
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["mode"], "STOP");
    assert_eq!(v["program"]["phase"], "idle");
    assert!(!v["scan"]["tasks"].as_array().unwrap().is_empty());
    assert_eq!(v["watchdog"], "ok");
}

#[tokio::test]
async fn sim_from_run_conflict() {
    let (app, state) = app_auth();
    bring_up_run(app.clone(), &state).await;
    let (status, _) = send(
        app,
        post_json("/api/v1/mode", Some(OPERATOR), r#"{"mode":"SIM"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn unknown_mode_400() {
    let (app, _) = app_auth();
    let (status, _) = send(
        app,
        post_json("/api/v1/mode", Some(OPERATOR), r#"{"mode":"FAULT"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
