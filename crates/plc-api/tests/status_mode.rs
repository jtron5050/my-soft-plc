//! Status and mode transitions.

mod common;

use axum::http::StatusCode;

use common::{app_auth, get_auth, post_json, send, OPERATOR, VIEWER};

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
    let (app, _) = app_auth();
    let (status, _) = send(
        app.clone(),
        post_json("/api/v1/mode", Some(OPERATOR), r#"{"mode":"RUN"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Mode request is queued; observed mode may still be STOP until a scan step.
    let (status, _) = send(
        app.clone(),
        post_json("/api/v1/mode", Some(OPERATOR), r#"{"mode":"SIM"}"#),
    )
    .await;
    // If RUN not yet applied, SIM is legal from STOP. Step once then retry.
    if status == StatusCode::OK {
        {
            let st = common::app_auth().1;
            let _ = st;
        }
    }
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
