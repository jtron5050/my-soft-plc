//! Device config GET/PUT/PATCH.

use axum::extract::State;
use axum::Json;
use plc_auth::{AuditAction, AuthService, Permission};
use plc_config::{validate, DeviceConfig};
use plc_types::OperatingMode;
use serde_json::Value;

use crate::auth::Authed;
use crate::dto::ConfigWriteResponse;
use crate::error::ApiError;
use crate::state::AppState;

/// `GET /api/v1/config`.
pub async fn get(
    State(state): State<AppState>,
    authed: Authed,
) -> Result<Json<DeviceConfig>, ApiError> {
    authed.require(&state, Permission::ConfigRead)?;
    let cfg = state.config.read().expect("config").clone();
    Ok(Json(redact_secrets(cfg)))
}

/// `PUT /api/v1/config`.
pub async fn put(
    State(state): State<AppState>,
    authed: Authed,
    Json(body): Json<DeviceConfig>,
) -> Result<Json<ConfigWriteResponse>, ApiError> {
    authed.require(&state, Permission::ConfigWrite)?;
    apply_config(&state, &authed, body)
}

/// `PATCH /api/v1/config` (RFC 7396 merge patch).
pub async fn patch(
    State(state): State<AppState>,
    authed: Authed,
    Json(patch): Json<Value>,
) -> Result<Json<ConfigWriteResponse>, ApiError> {
    authed.require(&state, Permission::ConfigWrite)?;
    let current = state.config.read().expect("config").clone();
    let base = serde_json::to_value(&current).map_err(|e| ApiError::internal(e.to_string()))?;
    let merged = merge(base, patch);
    let cfg: DeviceConfig = serde_json::from_value(merged)
        .map_err(|e| ApiError::bad_request("config", e.to_string()))?;
    apply_config(&state, &authed, cfg)
}

fn apply_config(
    state: &AppState,
    authed: &Authed,
    cfg: DeviceConfig,
) -> Result<Json<ConfigWriteResponse>, ApiError> {
    if state.scan_handle.mode() != OperatingMode::Stop {
        return Err(ApiError::conflict(
            "mode",
            "config write requires mode=STOP",
        ));
    }
    let cfg = validate(cfg)?;
    let old = state.config.read().expect("config").clone();
    let restart_required = needs_restart(&old, &cfg);
    let auth = AuthService::from_config(&cfg.auth, &cfg.limits)
        .map_err(|e| ApiError::bad_request("config", e.to_string()))?;
    if let Some(path) = state.config_path.as_ref() {
        plc_config::save_to_path(path, &cfg)?;
    }
    {
        let mut rt = state.runtime.lock().expect("runtime");
        let mut rc = rt.config().clone();
        rc.require_signature = cfg.program.require_signature;
        rt.set_config(rc);
    }
    *state.auth.write().expect("auth") = auth;
    *state.config.write().expect("config") = cfg;
    state.record(
        &authed.principal.id,
        AuditAction::ConfigWrite,
        format!("restart_required={restart_required}"),
        Some(authed.addr),
    );
    Ok(Json(ConfigWriteResponse { restart_required }))
}

/// Listener bind, TLS paths, body-limit, scan, and I/O drivers are captured at
/// process start (router / acceptor); those writes persist but need a restart.
fn needs_restart(old: &DeviceConfig, cfg: &DeviceConfig) -> bool {
    old.scan != cfg.scan
        || old.io.drivers != cfg.io.drivers
        || old.limits.max_package_bytes != cfg.limits.max_package_bytes
        || old.rest.bind != cfg.rest.bind
        || old.auth.tls_cert_path != cfg.auth.tls_cert_path
        || old.auth.tls_key_path != cfg.auth.tls_key_path
        || old.auth.client_ca_path != cfg.auth.client_ca_path
}

fn redact_secrets(mut cfg: DeviceConfig) -> DeviceConfig {
    cfg.auth.tls_key_path.clear();
    for p in &mut cfg.auth.principals {
        p.token_sha256 = None;
        p.cert_sha256 = None;
    }
    cfg
}

fn merge(base: Value, patch: Value) -> Value {
    match (base, patch) {
        (Value::Object(mut a), Value::Object(b)) => {
            for (k, v) in b {
                if v.is_null() {
                    a.remove(&k);
                } else {
                    let next = match a.get(&k) {
                        Some(cur) if cur.is_object() && v.is_object() => merge(cur.clone(), v),
                        _ => v,
                    };
                    a.insert(k, next);
                }
            }
            Value::Object(a)
        }
        (_, p) => p,
    }
}
