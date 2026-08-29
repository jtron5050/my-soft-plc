//! Status resources.

use axum::extract::State;
use axum::Json;
use plc_auth::Permission;
use plc_types::OperatingMode;

use crate::auth::Authed;
use crate::dto::{
    mode_wire, phase_wire, IoStatusBody, ProgramStatusBody, ScanBody, StatusBody, TaskTimingBody,
};
use crate::error::ApiError;
use crate::state::AppState;

/// `GET /api/v1/status`.
pub async fn status(
    State(state): State<AppState>,
    authed: Authed,
) -> Result<Json<StatusBody>, ApiError> {
    authed.require(&state, Permission::StatusRead)?;
    Ok(Json(build_status(&state)))
}

/// `GET /api/v1/status/tasks`.
pub async fn tasks(
    State(state): State<AppState>,
    authed: Authed,
) -> Result<Json<ScanBody>, ApiError> {
    authed.require(&state, Permission::StatusRead)?;
    Ok(Json(build_status(&state).scan))
}

/// `GET /api/v1/status/io`.
pub async fn io(
    State(state): State<AppState>,
    authed: Authed,
) -> Result<Json<IoStatusBody>, ApiError> {
    authed.require(&state, Permission::StatusRead)?;
    Ok(Json(build_status(&state).io))
}

/// Snapshot status (brief runtime lock; no filesystem I/O).
pub fn build_status(state: &AppState) -> StatusBody {
    let rt = state.runtime.lock().expect("runtime");
    let snap = rt.engine().status();
    let watchdog = if snap.mode == OperatingMode::Fault {
        "fault"
    } else {
        "ok"
    };
    StatusBody {
        mode: mode_wire(snap.mode),
        program: ProgramStatusBody {
            phase: phase_wire(snap.phase),
            current: rt.current_info().map(Into::into),
            armed: rt.armed_info().map(Into::into),
        },
        scan: ScanBody {
            tasks: snap
                .tasks
                .iter()
                .map(|t| TaskTimingBody {
                    name: t.name.clone(),
                    period_ms: t.period_ms,
                    last_us: t.last_us,
                    max_us: t.max_us,
                    overruns: t.overruns,
                })
                .collect(),
        },
        watchdog,
        io: IoStatusBody {
            degraded: snap.io_degraded,
            modules_bad: Vec::new(),
        },
        uptime_s: state.started.elapsed().as_secs(),
    }
}
