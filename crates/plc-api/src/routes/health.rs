//! Liveness.

use axum::Json;

use crate::dto::HealthBody;

/// `GET /api/v1/health` — unauthenticated.
pub async fn health() -> Json<HealthBody> {
    Json(HealthBody { status: "ok" })
}
