//! REST management API (`https://<device>:8443/api/v1`).
//!
//! Architecture PR-12: axum + tokio, OpenAPI in `docs/openapi/openapi.yaml`.
//! The RT scan thread is not this crate — handlers take brief locks on
//! [`plc_runtime::Runtime`] and never import scan-path dependencies of tokio
//! into RT crates.

#![forbid(unsafe_code)]

use std::sync::atomic::Ordering;

use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;

mod auth;
mod dto;
mod error;
mod events;
mod force_limit;
mod listen;
mod program_store;
mod routes;
mod state;
mod tls;

pub use error::{ApiError, ErrorBody};
pub use listen::serve;
pub use program_store::{ProgramStore, StoredMeta};
pub use state::AppState;
pub use tls::{listen_mode, ListenMode};

/// Build the `/api/v1` router.
pub fn router(state: AppState) -> Router {
    let max = state.max_package_bytes();
    let api = Router::new()
        .route("/health", get(routes::health::health))
        .route("/status", get(routes::status::status))
        .route("/status/tasks", get(routes::status::tasks))
        .route("/status/io", get(routes::status::io))
        .route(
            "/config",
            get(routes::config::get)
                .put(routes::config::put)
                .patch(routes::config::patch),
        )
        .route(
            "/programs",
            get(routes::programs::list).post(routes::programs::upload),
        )
        .route(
            "/programs/{id}",
            get(routes::programs::get).delete(routes::programs::delete),
        )
        .route("/programs/{id}/arm", post(routes::programs::arm))
        .route("/programs/{id}/activate", post(routes::programs::activate))
        .route("/mode", post(routes::mode::set_mode))
        .route("/tags", get(routes::tags::list))
        .route(
            "/tags/{name}",
            get(routes::tags::get).put(routes::tags::put),
        )
        .route("/metrics", get(routes::metrics::metrics))
        .route("/diagnostics/events", get(routes::diagnostics::events))
        .route("/diagnostics/audit", get(routes::diagnostics::audit));

    Router::new()
        .nest("/api/v1", api)
        .layer(DefaultBodyLimit::max(max))
        .layer(middleware::from_fn_with_state(state.clone(), count_mw))
        .with_state(state)
}

async fn count_mw(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let res = next.run(req).await;
    if res.status() == StatusCode::OK
        || res.status() == StatusCode::CREATED
        || res.status() == StatusCode::ACCEPTED
        || res.status() == StatusCode::NO_CONTENT
    {
        state.http_ok.fetch_add(1, Ordering::Relaxed);
    } else {
        state.http_err.fetch_add(1, Ordering::Relaxed);
    }
    res
}
