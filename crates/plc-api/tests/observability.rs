//! Prometheus scan stats and the diagnostics ring feed.

mod common;

use axum::http::StatusCode;
use common::{app_auth, get_auth, send, VIEWER};
use plc_scan::ModeRequest;

#[tokio::test]
async fn metrics_include_scan_stats_and_overrun_is_a_diagnostic() {
    let (app, state) = app_auth();
    {
        let mut rt = state.runtime.lock().expect("runtime");
        rt.engine_mut()
            .set_min_duration_us("main", Some(60_000))
            .expect("task main");
        rt.engine_mut().request_mode(ModeRequest::Run);
        rt.step().expect("step");
    }

    let (status, body) = send(app.clone(), get_auth("/api/v1/metrics", VIEWER)).await;
    assert_eq!(status, StatusCode::OK);
    let text = String::from_utf8(body).expect("utf-8");
    for series in [
        "softplc_task_last_duration_us",
        "softplc_task_max_duration_us",
        "softplc_task_avg_duration_us",
        "softplc_task_duration_us_bucket",
        "softplc_task_duration_us_sum",
        "softplc_task_duration_us_count",
        "softplc_fault",
        "softplc_task_overruns_total",
    ] {
        assert!(text.contains(series), "missing {series} in {text}");
    }
    assert!(text.contains("softplc_task_overruns_total{task=\"main\"} 1"));

    let (status, body) = send(app, get_auth("/api/v1/diagnostics/events", VIEWER)).await;
    assert_eq!(status, StatusCode::OK);
    let events = String::from_utf8(body).expect("utf-8");
    assert!(
        events.contains("logic_overrun"),
        "expected logic_overrun in {events}"
    );
}
