//! Load and store device configuration documents.

use std::fs;
use std::io::Read;
use std::path::Path;

use crate::error::ConfigError;
use crate::schema::DeviceConfig;
use crate::validate::validate;

/// On-disk / wire format for configuration documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    /// YAML (preferred for human-edited samples).
    Yaml,
    /// JSON (REST / tooling).
    Json,
}

impl ConfigFormat {
    /// Infer format from file extension (`.yaml`/`.yml` → YAML, `.json` → JSON).
    #[must_use]
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension().and_then(|e| e.to_str()) {
            Some("yaml" | "yml") => Some(Self::Yaml),
            Some("json") => Some(Self::Json),
            _ => None,
        }
    }
}

/// Load, parse, and validate configuration from a filesystem path.
pub fn load_from_path(path: &Path) -> Result<DeviceConfig, ConfigError> {
    let format = ConfigFormat::from_path(path).ok_or_else(|| {
        ConfigError::validation(format!(
            "unsupported config extension for {}",
            path.display()
        ))
    })?;
    let text = fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    load_from_str(&text, format)
}

/// Load from an arbitrary reader (tests / stdin). Format must be provided.
pub fn load_from_reader<R: Read>(
    mut reader: R,
    format: ConfigFormat,
) -> Result<DeviceConfig, ConfigError> {
    let mut text = String::new();
    reader
        .read_to_string(&mut text)
        .map_err(|source| ConfigError::Io {
            path: Path::new("<reader>").to_path_buf(),
            source,
        })?;
    load_from_str(&text, format)
}

/// Parse and validate a configuration string.
pub fn load_from_str(text: &str, format: ConfigFormat) -> Result<DeviceConfig, ConfigError> {
    let cfg: DeviceConfig = match format {
        ConfigFormat::Yaml => serde_yaml::from_str(text)?,
        ConfigFormat::Json => serde_json::from_str(text)?,
    };
    validate(cfg)
}

/// Serialize and write configuration (temp + rename not required for PR-02 unit use).
pub fn save_to_path(path: &Path, cfg: &DeviceConfig) -> Result<(), ConfigError> {
    let format = ConfigFormat::from_path(path).ok_or_else(|| {
        ConfigError::validation(format!(
            "unsupported config extension for {}",
            path.display()
        ))
    })?;
    let text = match format {
        ConfigFormat::Yaml => serde_yaml::to_string(cfg)?,
        ConfigFormat::Json => serde_json::to_string_pretty(cfg)?,
    };
    fs::write(path, text).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })
}
