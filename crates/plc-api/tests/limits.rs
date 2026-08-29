//! Upload / force limits.

mod common;

use axum::http::StatusCode;

use common::{app_auth, get_auth, send, VIEWER};

#[tokio::test]
async fn metrics_and_diagnostics() {
    let (app, _) = app_auth();
    let (status, body) = send(app.clone(), get_auth("/api/v1/metrics", VIEWER)).await;
    assert_eq!(status, StatusCode::OK);
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("softplc_http_requests_total"));

    let (status, _) = send(app.clone(), get_auth("/api/v1/diagnostics/events", VIEWER)).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(app, get_auth("/api/v1/diagnostics/audit", VIEWER)).await;
    assert_eq!(status, StatusCode::OK);
}
