//! Operator mode.

use axum::extract::State;
use axum::Json;
use plc_auth::{AuditAction, Permission};
use plc_scan::ModeRequest;
use plc_types::{OperatingMode, ProgramPhase};

use crate::auth::Authed;
use crate::dto::{mode_wire, ModeBody, ModeResponse};
use crate::error::ApiError;
use crate::state::AppState;

/// `POST /api/v1/mode`.
pub async fn set_mode(
    State(state): State<AppState>,
    authed: Authed,
    Json(body): Json<ModeBody>,
) -> Result<Json<ModeResponse>, ApiError> {
    authed.require(&state, Permission::ModeWrite)?;
    let req = parse_mode_request(&body.mode)?;
    if state.hooks.phase() == ProgramPhase::Swapping {
        return Err(ApiError::conflict(
            "phase_swapping",
            "mode change while swapping",
        ));
    }
    let current = state.scan_handle.mode();
    precheck(current, req)?;
    state.scan_handle.request_mode(req);
    state.record(
        &authed.principal.id,
        AuditAction::ModeChange,
        body.mode.clone(),
        Some(authed.addr),
    );
    Ok(Json(ModeResponse {
        requested: body.mode,
        mode: mode_wire(state.scan_handle.mode()),
    }))
}

fn parse_mode_request(s: &str) -> Result<ModeRequest, ApiError> {
    match s {
        s if s.eq_ignore_ascii_case("STOP") => Ok(ModeRequest::Stop),
        s if s.eq_ignore_ascii_case("RUN") => Ok(ModeRequest::Run),
        s if s.eq_ignore_ascii_case("SIM") => Ok(ModeRequest::Sim),
        s if s.eq_ignore_ascii_case("FAULT_RESET") => Ok(ModeRequest::FaultReset),
        _ => Err(ApiError::bad_request(
            "validation",
            format!("unknown mode '{s}' (RUN|STOP|FAULT_RESET|SIM)"),
        )),
    }
}

fn precheck(current: OperatingMode, req: ModeRequest) -> Result<(), ApiError> {
    match (current, req) {
        (OperatingMode::Fault, ModeRequest::Stop) => Err(ApiError::conflict(
            "fault",
            "STOP while FAULT (FAULT_RESET first)",
        )),
        (OperatingMode::Fault, ModeRequest::Run) => Err(ApiError::conflict(
            "fault",
            "RUN while FAULT (FAULT_RESET first)",
        )),
        (OperatingMode::Fault, ModeRequest::Sim) => {
            Err(ApiError::conflict("fault", "SIM while FAULT"))
        }
        (OperatingMode::Run, ModeRequest::Sim) => {
            Err(ApiError::conflict("conflict", "SIM from RUN"))
        }
        (_, ModeRequest::FaultReset) if current != OperatingMode::Fault => Err(ApiError::conflict(
            "conflict",
            "FAULT_RESET while not FAULT",
        )),
        _ => Ok(()),
    }
}
