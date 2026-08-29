//! Tag dictionary and force writes.

use std::time::Instant;

use axum::extract::{Path, State};
use axum::Json;
use plc_auth::{AuditAction, Permission};

use crate::auth::Authed;
use crate::dto::{
    kind_wire, parse_force_value, TagDictEntry, TagListBody, TagReadBody, TagWriteBody,
    TagWriteResponse,
};
use crate::error::ApiError;
use crate::state::AppState;

/// `GET /api/v1/tags`.
pub async fn list(
    State(state): State<AppState>,
    authed: Authed,
) -> Result<Json<TagListBody>, ApiError> {
    authed.require(&state, Permission::TagRead)?;
    let rt = state.runtime.lock().expect("runtime");
    let tags = rt
        .tag_names()
        .into_iter()
        .map(|t| TagDictEntry {
            name: t.name,
            ty: t.ty.as_str().to_string(),
            kind: kind_wire(t.kind),
            slot: t.slot,
        })
        .collect();
    Ok(Json(TagListBody { tags }))
}

/// `GET /api/v1/tags/{name}`.
pub async fn get(
    State(state): State<AppState>,
    authed: Authed,
    Path(name): Path<String>,
) -> Result<Json<TagReadBody>, ApiError> {
    authed.require(&state, Permission::TagRead)?;
    let rt = state.runtime.lock().expect("runtime");
    let view = rt.read_tag(&name)?;
    Ok(Json(TagReadBody::from(&view)))
}

/// `PUT /api/v1/tags/{name}`.
pub async fn put(
    State(state): State<AppState>,
    authed: Authed,
    Path(name): Path<String>,
    Json(body): Json<TagWriteBody>,
) -> Result<Json<TagWriteResponse>, ApiError> {
    authed.require(&state, Permission::TagForce)?;
    {
        let mut lim = state.force_limit.lock().expect("force");
        if let Err(secs) = lim.check(Instant::now()) {
            return Err(ApiError::rate_limited(
                "force_limit",
                "max 100 forced tag ops/min",
                secs,
            ));
        }
    }
    let ty = {
        let rt = state.runtime.lock().expect("runtime");
        let view = rt.read_tag(&name)?;
        view.type_name().to_string()
    };
    let value =
        parse_force_value(&ty, &body.value).map_err(|m| ApiError::bad_request("validation", m))?;
    {
        let mut rt = state.runtime.lock().expect("runtime");
        rt.force_tag(&name, value)?;
    }
    state.record(
        &authed.principal.id,
        AuditAction::TagForce,
        name,
        Some(authed.addr),
    );
    Ok(Json(TagWriteResponse { forced: true }))
}
