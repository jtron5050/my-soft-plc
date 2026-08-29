//! In-memory diagnostics and audit export.

use axum::extract::{Query, State};
use axum::Json;
use plc_auth::Permission;
use serde::Serialize;

use crate::auth::Authed;
use crate::dto::PageQuery;
use crate::error::ApiError;
use crate::events::DiagEvent;
use crate::state::AppState;

/// `GET /api/v1/diagnostics/events`.
pub async fn events(
    State(state): State<AppState>,
    authed: Authed,
    Query(q): Query<PageQuery>,
) -> Result<Json<Vec<DiagEvent>>, ApiError> {
    authed.require(&state, Permission::DiagnosticsRead)?;
    let limit = q.limit.unwrap_or(100).min(1000) as usize;
    let cursor = q.cursor.unwrap_or(0);
    let items: Vec<_> = state
        .events
        .snapshot()
        .into_iter()
        .filter(|e| e.seq > cursor)
        .take(limit)
        .collect();
    Ok(Json(items))
}

/// Audit row for JSON export.
#[derive(Debug, Serialize)]
pub struct AuditRow {
    /// Index in the ring (oldest = 0).
    pub seq: u64,
    /// Unix seconds.
    pub unix_secs: u64,
    /// Principal.
    pub principal_id: String,
    /// Action name.
    pub action: String,
    /// Detail.
    pub detail: String,
}

/// `GET /api/v1/diagnostics/audit`.
pub async fn audit(
    State(state): State<AppState>,
    authed: Authed,
    Query(q): Query<PageQuery>,
) -> Result<Json<Vec<AuditRow>>, ApiError> {
    authed.require(&state, Permission::AuditRead)?;
    let limit = q.limit.unwrap_or(100).min(1000) as usize;
    let cursor = q.cursor.unwrap_or(0);
    let rows: Vec<AuditRow> = state
        .audit
        .events()
        .into_iter()
        .enumerate()
        .filter(|(i, _)| *i as u64 >= cursor)
        .take(limit)
        .map(|(i, e)| AuditRow {
            seq: i as u64,
            unix_secs: e.unix_secs,
            principal_id: e.principal_id,
            action: format!("{:?}", e.action),
            detail: e.detail,
        })
        .collect();
    Ok(Json(rows))
}
