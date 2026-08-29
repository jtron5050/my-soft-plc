//! Prometheus text exposition (thin; histograms in PR-18).

use std::sync::atomic::Ordering;

use axum::extract::State;
use axum::http::{header, HeaderValue};
use axum::response::IntoResponse;
use plc_auth::Permission;

use crate::auth::Authed;
use crate::error::ApiError;
use crate::state::AppState;

/// `GET /api/v1/metrics`.
pub async fn metrics(
    State(state): State<AppState>,
    authed: Authed,
) -> Result<impl IntoResponse, ApiError> {
    authed.require(&state, Permission::MetricsRead)?;
    let mut body = String::from("# TYPE softplc_http_requests_total counter\n");
    body.push_str(&format!(
        "softplc_http_requests_total{{result=\"ok\"}} {}\n",
        state.http_ok.load(Ordering::Relaxed)
    ));
    body.push_str(&format!(
        "softplc_http_requests_total{{result=\"error\"}} {}\n",
        state.http_err.load(Ordering::Relaxed)
    ));
    {
        let rt = state.runtime.lock().expect("runtime");
        let snap = rt.engine().status();
        body.push_str("# TYPE softplc_telemetry_drops_total counter\n");
        body.push_str(&format!(
            "softplc_telemetry_drops_total {}\n",
            snap.telemetry_drops
        ));
        body.push_str("# TYPE softplc_mode_rejected_total counter\n");
        body.push_str(&format!(
            "softplc_mode_rejected_total {}\n",
            snap.mode_rejected
        ));
        body.push_str("# TYPE softplc_activate_deferred_total counter\n");
        body.push_str(&format!(
            "softplc_activate_deferred_total {}\n",
            snap.activate_deferred_count
        ));
        body.push_str("# TYPE softplc_task_overruns_total counter\n");
        for t in &snap.tasks {
            body.push_str(&format!(
                "softplc_task_overruns_total{{task=\"{}\"}} {}\n",
                t.name, t.overruns
            ));
        }
    }
    let mut res = body.into_response();
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
    );
    Ok(res)
}
