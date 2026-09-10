//! Tag dictionary and force.

mod common;

use axum::http::StatusCode;

use common::{
    app_auth, get_auth, pack_line, post_bytes, post_json, put_json, send, ENGINEER, OPERATOR,
    VIEWER,
};

#[tokio::test]
async fn tags_and_force() {
    let (app, _) = app_auth();
    let (status, _) = send(
        app.clone(),
        post_bytes("/api/v1/programs", Some(ENGINEER), pack_line()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = send(
        app.clone(),
        post_json("/api/v1/programs/line/arm", Some(ENGINEER), ""),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(app.clone(), get_auth("/api/v1/tags", VIEWER)).await;
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(v["tags"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["name"] == "Conveyor1/RunFwd"));

    let (status, _) = send(
        app.clone(),
        put_json(
            "/api/v1/tags/Conveyor1%2FRunFwd",
            Some(OPERATOR),
            r#"{"value":true}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(
        app.clone(),
        get_auth("/api/v1/tags/Conveyor1%2FRunFwd", VIEWER),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["forced"], true);
    assert_eq!(v["value"], true);

    let (status, _) = send(
        app,
        put_json("/api/v1/tags/Q0", Some(VIEWER), r#"{"value":true}"#),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn sim_input_inject_and_run_rejected() {
    use plc_scan::ModeRequest;
    use plc_types::OperatingMode;

    use common::{app_sim_inject, step_until_mode};

    let (app, state) = app_sim_inject();
    let (status, _) = send(
        app.clone(),
        post_bytes("/api/v1/programs", Some(ENGINEER), pack_line()),
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
    assert!(status == StatusCode::ACCEPTED || status == StatusCode::OK);
    {
        let mut rt = state.runtime.lock().unwrap();
        let _ = rt.step();
    }

    let (status, body) = send(
        app.clone(),
        put_json("/api/v1/tags/I0", Some(OPERATOR), r#"{"value":true}"#),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "{}",
        String::from_utf8_lossy(&body)
    );

    state.scan_handle.request_mode(ModeRequest::Sim);
    step_until_mode(&state, OperatingMode::Sim);

    let (status, body) = send(
        app.clone(),
        put_json("/api/v1/tags/I0", Some(OPERATOR), r#"{"value":true}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["forced"], false);

    state.scan_handle.request_mode(ModeRequest::Stop);
    step_until_mode(&state, OperatingMode::Stop);
    state.scan_handle.request_mode(ModeRequest::Run);
    step_until_mode(&state, OperatingMode::Run);
    let (status, _) = send(
        app,
        put_json("/api/v1/tags/I0", Some(OPERATOR), r#"{"value":false}"#),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
