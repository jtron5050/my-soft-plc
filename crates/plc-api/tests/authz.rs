//! Authn/authz, lockout, rate limit.

mod common;

use axum::http::StatusCode;

use common::{app_auth, app_open, get, get_auth, post_json, send, ENGINEER, OPERATOR, VIEWER};

#[tokio::test]
async fn anonymous_admin_when_auth_not_required() {
    let (status, _) = send(app_open(), get("/api/v1/status")).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn missing_token_is_401_when_required() {
    let (app, _) = app_auth();
    let (status, _) = send(app, get("/api/v1/status")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn bad_token_is_401() {
    let (app, _) = app_auth();
    let (status, _) = send(app, get_auth("/api/v1/status", "nope")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn viewer_reads_status_forbidden_on_mode() {
    let (app, _) = app_auth();
    let (status, _) = send(app.clone(), get_auth("/api/v1/status", VIEWER)).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        app,
        post_json("/api/v1/mode", Some(VIEWER), r#"{"mode":"RUN"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn operator_cannot_activate() {
    let (app, _) = app_auth();
    let (status, _) = send(
        app,
        post_json("/api/v1/programs/line/activate", Some(OPERATOR), "{}"),
    )
    .await;
    assert!(status == StatusCode::FORBIDDEN || status == StatusCode::CONFLICT);
}

#[tokio::test]
async fn engineer_forbidden_not_needed_for_status() {
    let (app, _) = app_auth();
    let (status, _) = send(app, get_auth("/api/v1/status", ENGINEER)).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn lockout_after_five_failures() {
    let (app, _) = app_auth();
    for _ in 0..5 {
        let (status, _) = send(app.clone(), get_auth("/api/v1/status", "bad")).await;
        assert!(status == StatusCode::UNAUTHORIZED || status == StatusCode::TOO_MANY_REQUESTS);
    }
    let (status, _) = send(app, get_auth("/api/v1/status", "bad")).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}
