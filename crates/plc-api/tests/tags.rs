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

    let (status, _) = send(
        app,
        put_json("/api/v1/tags/Q0", Some(VIEWER), r#"{"value":true}"#),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
