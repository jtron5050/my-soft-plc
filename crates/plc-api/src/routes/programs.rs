//! Program store / arm / activate.

use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use plc_auth::{AuditAction, Permission};
use plc_runtime::ActivateRequest;
use plc_types::ProgramPhase as Phase;
use tokio::sync::OwnedSemaphorePermit;
use uuid::Uuid;

use crate::auth::Authed;
use crate::dto::{ActivateAccepted, ActivateQuery, ProgramDetailBody, RetainReportBody};
use crate::error::ApiError;
use crate::program_store::StoredMeta;
use crate::routes::status::build_status;
use crate::state::{ActivateJob, AppState};

/// Acquired before the body is read so a second client gets 429 without
/// buffering `max_package_bytes`.
pub(crate) struct UploadPermit {
    /// Held until the handler returns so the slot stays occupied.
    _permit: OwnedSemaphorePermit,
}

impl FromRequestParts<AppState> for UploadPermit {
    type Rejection = ApiError;

    async fn from_request_parts(
        _parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let permit = state.upload_sem.clone().try_acquire_owned().map_err(|_| {
            ApiError::rate_limited("upload_busy", "concurrent upload already in progress", 1)
        })?;
        Ok(Self { _permit: permit })
    }
}

/// Snapshot current/armed ids under the runtime mutex, then write pointer
/// files after the lock is dropped.
fn sync_program_pointers(state: &AppState) {
    let (current, armed) = {
        let rt = state.runtime.lock().expect("runtime");
        (
            rt.current_info().map(|p| p.id.clone()),
            rt.armed_info().map(|p| p.id.clone()),
        )
    };
    let _ = state.store.set_pointer("current", current.as_deref());
    let _ = state.store.set_pointer("armed", armed.as_deref());
}

/// `GET /api/v1/programs`.
pub async fn list(
    State(state): State<AppState>,
    authed: Authed,
) -> Result<Json<Vec<StoredMeta>>, ApiError> {
    authed.require(&state, Permission::ProgramRead)?;
    Ok(Json(state.store.list()?))
}

/// `POST /api/v1/programs` — store only.
pub async fn upload(
    State(state): State<AppState>,
    authed: Authed,
    _permit: UploadPermit,
    body: Bytes,
) -> Result<(StatusCode, Json<StoredMeta>), ApiError> {
    authed.require(&state, Permission::ProgramUpload)?;
    let max = state.max_package_bytes();
    if body.len() > max {
        return Err(ApiError::too_large(body.len()));
    }
    let meta = state
        .store
        .put(&body, Some(&authed.principal.id), state.unix_secs())?;
    Ok((StatusCode::CREATED, Json(meta)))
}

/// `GET /api/v1/programs/{id}`.
pub async fn get(
    State(state): State<AppState>,
    authed: Authed,
    Path(id): Path<String>,
) -> Result<Json<ProgramDetailBody>, ApiError> {
    authed.require(&state, Permission::ProgramRead)?;
    let (meta, _) = state.store.get(&id)?;
    let retain = {
        let rt = state.runtime.lock().expect("runtime");
        if rt.armed_info().is_some_and(|p| p.id == id) {
            rt.last_arm().map(RetainReportBody::from)
        } else {
            None
        }
    };
    Ok(Json(ProgramDetailBody {
        id: meta.id,
        version: meta.version,
        build_id: meta.build_id,
        compatibility_hash: meta.compatibility_hash,
        size: meta.size,
        uploader: meta.uploader,
        retain,
    }))
}

/// `DELETE /api/v1/programs/{id}`.
pub async fn delete(
    State(state): State<AppState>,
    authed: Authed,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    authed.require(&state, Permission::ProgramDelete)?;
    {
        let rt = state.runtime.lock().expect("runtime");
        if rt.current_info().is_some_and(|p| p.id == id) {
            return Err(ApiError::conflict(
                "program_current",
                "cannot delete the current program",
            ));
        }
        if rt.armed_info().is_some_and(|p| p.id == id) {
            return Err(ApiError::conflict(
                "program_armed",
                "cannot delete the armed program",
            ));
        }
    }
    state.store.delete(&id)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/programs/{id}/arm`.
pub async fn arm(
    State(state): State<AppState>,
    authed: Authed,
    Path(id): Path<String>,
) -> Result<Json<ProgramDetailBody>, ApiError> {
    authed.require(&state, Permission::ProgramArm)?;
    let (meta, bytes) = state.store.get(&id)?;
    let _guard = state.arm_lock.lock().await;
    {
        let rt = state.runtime.lock().expect("runtime");
        if let Some(armed) = rt.armed_info() {
            if armed.id == meta.id && armed.compatibility_hash == meta.compatibility_hash {
                let retain = rt.last_arm().map(RetainReportBody::from);
                return Ok(Json(ProgramDetailBody {
                    id: meta.id,
                    version: meta.version,
                    build_id: meta.build_id,
                    compatibility_hash: meta.compatibility_hash,
                    size: meta.size,
                    uploader: meta.uploader,
                    retain,
                }));
            }
        }
    }
    let ctx = {
        let mut rt = state.runtime.lock().expect("runtime");
        rt.begin_arm()?
    };
    let prepared = match plc_runtime::Runtime::prepare_arm(&bytes, &ctx) {
        Ok(p) => p,
        Err(e) => {
            state.runtime.lock().expect("runtime").abort_arm();
            return Err(e.into());
        }
    };
    let report = {
        let mut rt = state.runtime.lock().expect("runtime");
        match rt.commit_arm(prepared) {
            Ok(r) => {
                if let Some(up) = &meta.uploader {
                    rt.set_uploader(up.clone());
                }
                r
            }
            Err(e) => {
                rt.abort_arm();
                return Err(e.into());
            }
        }
    };
    sync_program_pointers(&state);
    state.record(
        &authed.principal.id,
        AuditAction::ProgramArm,
        id.clone(),
        Some(authed.addr),
    );
    Ok(Json(ProgramDetailBody {
        id: meta.id,
        version: meta.version,
        build_id: meta.build_id,
        compatibility_hash: meta.compatibility_hash,
        size: meta.size,
        uploader: meta.uploader,
        retain: Some(RetainReportBody::from(&report)),
    }))
}

/// `POST /api/v1/programs/{id}/activate`.
pub async fn activate(
    State(state): State<AppState>,
    authed: Authed,
    Path(id): Path<String>,
    Query(q): Query<ActivateQuery>,
) -> Result<impl IntoResponse, ApiError> {
    authed.require(&state, Permission::ProgramActivate)?;
    let store_uploader = state
        .store
        .get(&id)
        .ok()
        .and_then(|(m, _)| m.uploader)
        .unwrap_or_default();
    let outcome = {
        let _guard = state.arm_lock.lock().await;
        {
            let auth = state.auth.read().expect("auth");
            let rt = state.runtime.lock().expect("runtime");
            match rt.armed_info() {
                None => {
                    return Err(ApiError::conflict("not_armed", "activate while not armed"));
                }
                Some(p) if p.id != id => {
                    return Err(ApiError::conflict(
                        "wrong_program",
                        format!("armed program is '{}', not '{id}'", p.id),
                    ));
                }
                Some(_) => {}
            }
            if !auth.dual_control_ok(&store_uploader, &authed.principal.id) {
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "forbidden",
                    "dual_control",
                    "activate requires a different principal than the uploader",
                ));
            }
            if rt.phase() == Phase::Validating {
                return Err(ApiError::conflict(
                    "phase_validating",
                    "activate while validating",
                ));
            }
            if rt.phase() == Phase::Swapping {
                return Err(ApiError::conflict(
                    "phase_swapping",
                    "activate while swapping",
                ));
            }
            if rt.activate_requested() {
                return Err(ApiError::conflict(
                    "activate_pending",
                    "activate already requested",
                ));
            }
        }
        let mut rt = state.runtime.lock().expect("runtime");
        rt.activate()?
    };
    state.record(
        &authed.principal.id,
        AuditAction::ProgramActivate,
        id.clone(),
        Some(authed.addr),
    );
    match outcome {
        ActivateRequest::NoOp => {
            sync_program_pointers(&state);
            Ok((
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "idle",
                    "program_id": id,
                })),
            )
                .into_response())
        }
        ActivateRequest::Pending => {
            let job_id = Uuid::new_v4().to_string();
            *state.activate_job.lock().expect("job") = Some(ActivateJob {
                job_id: job_id.clone(),
                program_id: id.clone(),
            });
            if let Some(wait_ms) = q.wait_ms.filter(|n| *n > 0) {
                if wait_for_idle(&state, wait_ms).await {
                    sync_program_pointers(&state);
                    let body = build_status(&state);
                    return Ok((StatusCode::OK, Json(body)).into_response());
                }
            }
            Ok((
                StatusCode::ACCEPTED,
                Json(ActivateAccepted {
                    job_id,
                    status: "pending",
                }),
            )
                .into_response())
        }
    }
}

async fn wait_for_idle(state: &AppState, wait_ms: u64) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(wait_ms);
    loop {
        {
            let rt = state.runtime.lock().expect("runtime");
            if rt.phase() != Phase::Swapping
                && (rt.phase() == Phase::Idle || rt.current_info().is_some())
                && rt.armed_info().is_none()
            {
                return true;
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
