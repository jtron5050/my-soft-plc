//! HTTP error type mapped from auth / runtime / config failures.

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use plc_auth::AuthError;
use plc_config::ConfigError;
use plc_package::PackageError;
use plc_runtime::RuntimeError;
use serde::Serialize;

/// JSON error body frozen in OpenAPI.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorBody {
    /// Stable class (`unauthenticated`, `conflict`, …).
    pub error: &'static str,
    /// Human-readable detail.
    pub message: String,
    /// Machine code (phase, resource, …).
    pub code: &'static str,
}

/// REST failure.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    body: ErrorBody,
    retry_after: Option<u64>,
}

impl ApiError {
    /// Build from parts.
    #[must_use]
    pub fn new(
        status: StatusCode,
        error: &'static str,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            status,
            body: ErrorBody {
                error,
                message: message.into(),
                code,
            },
            retry_after: None,
        }
    }

    /// 409 Conflict.
    #[must_use]
    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", code, message)
    }

    /// 404 Not Found.
    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", "not_found", message)
    }

    /// 400 Bad Request.
    #[must_use]
    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", code, message)
    }

    /// 413 Payload Too Large.
    #[must_use]
    pub fn too_large(len: usize) -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too_large",
            "package_too_large",
            format!("upload exceeds limit ({len} bytes)"),
        )
    }

    /// 429 with `Retry-After`.
    #[must_use]
    pub fn rate_limited(code: &'static str, message: impl Into<String>, retry_after: u64) -> Self {
        let mut e = Self::new(StatusCode::TOO_MANY_REQUESTS, "rate_limited", code, message);
        e.retry_after = Some(retry_after.max(1));
        e
    }

    /// 500.
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "internal",
            message,
        )
    }
}

impl From<AuthError> for ApiError {
    fn from(err: AuthError) -> Self {
        match err {
            AuthError::Unauthenticated => Self::new(
                StatusCode::UNAUTHORIZED,
                "unauthenticated",
                "unauthenticated",
                "unauthenticated",
            ),
            AuthError::Forbidden {
                required,
                need,
                role,
            } => Self::new(
                StatusCode::FORBIDDEN,
                "forbidden",
                "forbidden",
                format!("permission {required} requires role {need}, have {role}"),
            ),
            AuthError::Locked { retry_after_secs } => Self::rate_limited(
                "locked",
                format!("locked; retry after {retry_after_secs}s"),
                retry_after_secs,
            ),
            AuthError::RateLimited { retry_after_secs } => Self::rate_limited(
                "rate_limited",
                format!("rate limited; retry after {retry_after_secs}s"),
                retry_after_secs,
            ),
            AuthError::Config(msg) => Self::internal(msg),
        }
    }
}

impl From<RuntimeError> for ApiError {
    fn from(err: RuntimeError) -> Self {
        match err {
            RuntimeError::Conflict { context } => {
                let code = if context.contains("swapping") {
                    "phase_swapping"
                } else if context.contains("validating") {
                    "phase_validating"
                } else if context.contains("activate") {
                    "activate_pending"
                } else if context.contains("not armed") {
                    "not_armed"
                } else {
                    "conflict"
                };
                Self::conflict(code, context)
            }
            RuntimeError::NotFound(msg) => Self::not_found(msg),
            RuntimeError::BadRequest(msg) => Self::bad_request("bad_request", msg),
            RuntimeError::Package(e) => package_err(&e),
            RuntimeError::Arm(_) | RuntimeError::Vm(_) | RuntimeError::Retain(_) => {
                Self::bad_request("validation", err.to_string())
            }
            RuntimeError::Scan(e) => {
                let msg = e.to_string();
                if msg.contains("invalid state")
                    || msg.contains("swapping")
                    || msg.contains("not armed")
                {
                    Self::conflict("conflict", msg)
                } else {
                    Self::bad_request("scan", msg)
                }
            }
        }
    }
}

fn package_err(err: &PackageError) -> ApiError {
    match err {
        PackageError::TooLarge(n) => ApiError::too_large(*n),
        _ => ApiError::bad_request("validation", err.to_string()),
    }
}

impl From<PackageError> for ApiError {
    fn from(err: PackageError) -> Self {
        package_err(&err)
    }
}

impl From<ConfigError> for ApiError {
    fn from(err: ConfigError) -> Self {
        Self::bad_request("config", err.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut res = (self.status, Json(self.body)).into_response();
        if let Some(secs) = self.retry_after {
            if let Ok(v) = HeaderValue::from_str(&secs.to_string()) {
                res.headers_mut().insert(header::RETRY_AFTER, v);
            }
        }
        res
    }
}
