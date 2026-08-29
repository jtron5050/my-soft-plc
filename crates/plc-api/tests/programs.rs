//! Store / arm / activate.

mod common;

use axum::http::StatusCode;
use plc_scan::ModeRequest;

use common::{
    app_auth, app_dual, delete_auth, get_auth, pack_line, post_bytes, post_json, send, ENGINEER,
    ENGINEER_B, VIEWER,
};

#[tokio::test]
async fn upload_arm_activate_flow() {
    let (app, state) = app_auth();
    let pkg = pack_line();
    let (status, body) = send(
        app.clone(),
        post_bytes("/api/v1/programs", Some(ENGINEER), pkg),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let meta: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(meta["id"], "line");

    let (status, body) = send(
        app.clone(),
        post_json("/api/v1/programs/line/arm", Some(ENGINEER), ""),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, _) = send(
        app.clone(),
        post_json("/api/v1/programs/line/arm", Some(ENGINEER), ""),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(
        app.clone(),
        post_json("/api/v1/programs/line/activate", Some(ENGINEER), ""),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let acc: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(acc["status"], "pending");
    assert!(acc["job_id"].as_str().unwrap().len() > 8);

    {
        let mut rt = state.runtime.lock().unwrap();
        rt.engine_mut().request_mode(ModeRequest::Run);
        for _ in 0..20 {
            let _ = rt.step();
        }
    }
    let (status, body) = send(app.clone(), get_auth("/api/v1/status", VIEWER)).await;
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["program"]["current"]["id"], "line");

    let (status, _) = send(app, delete_auth("/api/v1/programs/line", ENGINEER)).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn activate_not_armed_409() {
    let (app, _) = app_auth();
    let (status, _) = send(
        app,
        post_json("/api/v1/programs/line/activate", Some(ENGINEER), ""),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn invalid_arm_400() {
    let (app, _) = app_auth();
    let (status, _) = send(
        app.clone(),
        post_bytes(
            "/api/v1/programs",
            Some(ENGINEER),
            b"not-a-package".to_vec(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn dual_control_blocks_uploader() {
    let (app, _) = app_dual();
    let pkg = pack_line();
    let (status, _) = send(
        app.clone(),
        post_bytes("/api/v1/programs", Some(ENGINEER), pkg),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = send(
        app.clone(),
        post_json("/api/v1/programs/line/arm", Some(ENGINEER), ""),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        app.clone(),
        post_json("/api/v1/programs/line/activate", Some(ENGINEER), ""),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send(
        app,
        post_json("/api/v1/programs/line/activate", Some(ENGINEER_B), ""),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
}

#[tokio::test]
async fn concurrent_upload_429() {
    let (app, state) = app_auth();
    let _permit = state.upload_sem.acquire().await.unwrap();
    let (status, _) = send(
        app,
        post_bytes("/api/v1/programs", Some(ENGINEER), pack_line()),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn oversized_body_413() {
    let (app, _) = app_auth();
    let too_big = vec![0u8; 8 * 1024 * 1024 + 1];
    let (status, _) = send(app, post_bytes("/api/v1/programs", Some(ENGINEER), too_big)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn viewer_cannot_upload() {
    let (app, _) = app_auth();
    let (status, _) = send(
        app,
        post_bytes("/api/v1/programs", Some(VIEWER), pack_line()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
