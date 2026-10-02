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
    state.poll_scan_diagnostics();
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
    /// Monotonic file sequence.
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
    let rows = state
        .audit
        .page(cursor, limit)
        .map_err(|e| ApiError::internal(format!("audit read: {e}")))?
        .into_iter()
        .map(|row| AuditRow {
            seq: row.seq,
            unix_secs: row.unix_secs,
            principal_id: row.principal_id,
            action: row.action,
            detail: row.detail,
        })
        .collect();
    Ok(Json(rows))
}
