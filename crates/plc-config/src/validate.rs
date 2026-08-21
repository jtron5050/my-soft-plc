//! Semantic validation for [`DeviceConfig`](crate::DeviceConfig).

use crate::error::ConfigError;
use crate::schema::{DeviceConfig, ProfileKind, AUTH_ROLES, CONFIG_SCHEMA_VERSION};

/// Allowed I/O driver identifiers for pilot (KD-20).
const ALLOWED_DRIVERS: &[&str] = &["sim", "gpio", "modbus_tcp"];

/// Validate a loaded configuration.
///
/// Returns the same config on success so call sites can chain `load` → `validate`.
pub fn validate(cfg: DeviceConfig) -> Result<DeviceConfig, ConfigError> {
    if cfg.version != CONFIG_SCHEMA_VERSION {
        return Err(ConfigError::UnsupportedVersion(
            cfg.version,
            CONFIG_SCHEMA_VERSION,
        ));
    }

    if cfg.device.id.trim().is_empty() {
        return Err(ConfigError::validation("device.id must be non-empty"));
    }

    if cfg.scan.tasks.is_empty() {
        return Err(ConfigError::validation("scan.tasks must not be empty"));
    }

    let mut names = std::collections::BTreeSet::new();
    for task in &cfg.scan.tasks {
        if task.name.trim().is_empty() {
            return Err(ConfigError::validation("task.name must be non-empty"));
        }
        if !names.insert(task.name.as_str()) {
            return Err(ConfigError::validation(format!(
                "duplicate task.name '{}'",
                task.name
            )));
        }
        if task.period_ms == 0 {
            return Err(ConfigError::validation(format!(
                "task '{}': period_ms must be > 0",
                task.name
            )));
        }
        if task.period_ms > 60_000 {
            return Err(ConfigError::validation(format!(
                "task '{}': period_ms must be <= 60000",
                task.name
            )));
        }
        if task.entry.trim().is_empty() {
            return Err(ConfigError::validation(format!(
                "task '{}': entry must be non-empty",
                task.name
            )));
        }
    }

    if cfg.scan.overrun_limit == 0 {
        return Err(ConfigError::validation("scan.overrun_limit must be >= 1"));
    }

    if cfg.limits.max_package_bytes == 0 {
        return Err(ConfigError::validation(
            "limits.max_package_bytes must be > 0",
        ));
    }
    if cfg.limits.max_package_bytes > 64 * 1024 * 1024 {
        return Err(ConfigError::validation(
            "limits.max_package_bytes must be <= 64 MiB",
        ));
    }

    if cfg.telemetry.enabled {
        if cfg.telemetry.group_id.trim().is_empty() {
            return Err(ConfigError::validation(
                "telemetry.group_id must be non-empty when telemetry.enabled",
            ));
        }
        if cfg.telemetry.analog_period_ms == 0 {
            return Err(ConfigError::validation(
                "telemetry.analog_period_ms must be > 0",
            ));
        }
    }

    for d in &cfg.io.drivers {
        if !ALLOWED_DRIVERS.contains(&d.as_str()) {
            return Err(ConfigError::validation(format!(
                "io.drivers: unknown driver '{d}' (allowed: sim, gpio, modbus_tcp)"
            )));
        }
    }
    if cfg.io.drivers.is_empty() {
        return Err(ConfigError::validation(
            "io.drivers must list at least one driver",
        ));
    }

    if cfg.paths.programs.trim().is_empty() {
        return Err(ConfigError::validation("paths.programs must be non-empty"));
    }

    if cfg.limits.auth_fail_per_min == 0 {
        return Err(ConfigError::validation(
            "limits.auth_fail_per_min must be >= 1",
        ));
    }

    validate_auth(&cfg)?;

    // Prod profile soft checks (full refuse-insecure lands in PR-20).
    if cfg.profile == ProfileKind::Prod && !cfg.program.require_signature {
        return Err(ConfigError::validation(
            "profile=prod requires program.require_signature=true",
        ));
    }
    if cfg.profile == ProfileKind::Prod && !cfg.auth.required {
        return Err(ConfigError::validation(
            "profile=prod requires auth.required=true",
        ));
    }

    Ok(cfg)
}

fn validate_auth(cfg: &DeviceConfig) -> Result<(), ConfigError> {
    if cfg.auth.lockout_secs == 0 {
        return Err(ConfigError::validation("auth.lockout_secs must be >= 1"));
    }

    if cfg.auth.required && cfg.auth.principals.is_empty() {
        return Err(ConfigError::validation(
            "auth.required=true requires at least one principal",
        ));
    }

    let mut ids = std::collections::BTreeSet::new();
    for (i, p) in cfg.auth.principals.iter().enumerate() {
        let id = p.id.trim();
        if id.is_empty() {
            return Err(ConfigError::validation(format!(
                "auth.principals[{i}]: id must be non-empty"
            )));
        }
        if !ids.insert(id) {
            return Err(ConfigError::validation(format!(
                "auth.principals: duplicate id '{id}'"
            )));
        }
        if !AUTH_ROLES.contains(&p.role.as_str()) {
            return Err(ConfigError::validation(format!(
                "auth.principals '{id}': unknown role '{}' (allowed: viewer, operator, engineer, admin)",
                p.role
            )));
        }
        let token = nonempty_hash(p.token_sha256.as_deref());
        let cert = nonempty_hash(p.cert_sha256.as_deref());
        if token.is_none() && cert.is_none() {
            return Err(ConfigError::validation(format!(
                "auth.principals '{id}': at least one of token_sha256 or cert_sha256 is required"
            )));
        }
        if let Some(h) = token {
            check_sha256_hex(id, "token_sha256", h)?;
        }
        if let Some(h) = cert {
            check_sha256_hex(id, "cert_sha256", h)?;
        }
    }
    Ok(())
}

fn nonempty_hash(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

fn check_sha256_hex(id: &str, field: &str, value: &str) -> Result<(), ConfigError> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ConfigError::validation(format!(
            "auth.principals '{id}': {field} must be 64 hex characters"
        )));
    }
    Ok(())
}
