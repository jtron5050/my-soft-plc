//! Prometheus text exposition (0.0.4). Numbers come from the scan snapshot.

use std::sync::atomic::Ordering;

use axum::extract::State;
use axum::http::{header, HeaderValue};
use axum::response::IntoResponse;
use plc_auth::Permission;
use plc_scan::{ScanStatusSnapshot, TaskTiming, DURATION_BUCKET_BOUNDS_US};
use plc_types::OperatingMode;

use crate::auth::Authed;
use crate::error::ApiError;
use crate::state::AppState;

/// `GET /api/v1/metrics`.
pub async fn metrics(
    State(state): State<AppState>,
    authed: Authed,
) -> Result<impl IntoResponse, ApiError> {
    authed.require(&state, Permission::MetricsRead)?;
    state.poll_scan_diagnostics();
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
        write_task_series(&mut body, &snap);
        body.push_str("# TYPE softplc_fault gauge\n");
        let fault = u8::from(snap.mode == OperatingMode::Fault);
        body.push_str(&format!("softplc_fault {fault}\n"));
    }
    let mut res = body.into_response();
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
    );
    Ok(res)
}

fn write_task_series(body: &mut String, snap: &ScanStatusSnapshot) {
    body.push_str("# TYPE softplc_task_overruns_total counter\n");
    body.push_str("# TYPE softplc_task_last_duration_us gauge\n");
    body.push_str("# TYPE softplc_task_max_duration_us gauge\n");
    body.push_str("# TYPE softplc_task_avg_duration_us gauge\n");
    body.push_str("# TYPE softplc_task_duration_us histogram\n");
    for task in &snap.tasks {
        let name = prom_label(&task.name);
        body.push_str(&format!(
            "softplc_task_overruns_total{{task=\"{name}\"}} {}\n",
            task.overruns
        ));
        body.push_str(&format!(
            "softplc_task_last_duration_us{{task=\"{name}\"}} {}\n",
            task.last_us
        ));
        body.push_str(&format!(
            "softplc_task_max_duration_us{{task=\"{name}\"}} {}\n",
            task.max_us
        ));
        body.push_str(&format!(
            "softplc_task_avg_duration_us{{task=\"{name}\"}} {}\n",
            task.avg_us
        ));
        write_histogram(body, &name, task);
    }
}

fn write_histogram(body: &mut String, task: &str, timing: &TaskTiming) {
    let mut cumulative = 0u64;
    for (index, bound) in DURATION_BUCKET_BOUNDS_US.iter().enumerate() {
        cumulative = cumulative.saturating_add(timing.duration_buckets[index]);
        body.push_str(&format!(
            "softplc_task_duration_us_bucket{{task=\"{task}\",le=\"{bound}\"}} {cumulative}\n"
        ));
    }
    cumulative =
        cumulative.saturating_add(timing.duration_buckets[DURATION_BUCKET_BOUNDS_US.len()]);
    body.push_str(&format!(
        "softplc_task_duration_us_bucket{{task=\"{task}\",le=\"+Inf\"}} {cumulative}\n"
    ));
    body.push_str(&format!(
        "softplc_task_duration_us_sum{{task=\"{task}\"}} {}\n",
        timing.sum_us
    ));
    body.push_str(&format!(
        "softplc_task_duration_us_count{{task=\"{task}\"}} {}\n",
        timing.samples
    ));
}

fn prom_label(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '"' => out.push_str("\\\""),
            _ => out.push(ch),
        }
    }
    out
}
