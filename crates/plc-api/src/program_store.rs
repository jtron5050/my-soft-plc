//! On-disk program store: `{paths.programs}/<id>/package.spkg` + `meta.json`.

use std::fs;
use std::path::{Path, PathBuf};

use plc_package::{parse_spkg, validate_program_id};
use serde::{Deserialize, Serialize};

use crate::error::ApiError;

/// Persisted package metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredMeta {
    /// Manifest id.
    pub id: String,
    /// Manifest version.
    pub version: String,
    /// Manifest build id.
    pub build_id: String,
    /// Manifest compatibility hash.
    pub compatibility_hash: String,
    /// Byte size of `package.spkg`.
    pub size: u64,
    /// Principal who stored the package.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uploader: Option<String>,
    /// Wall-clock unix seconds.
    pub stored_at: u64,
}

/// Filesystem program store.
#[derive(Debug, Clone)]
pub struct ProgramStore {
    root: PathBuf,
}

impl ProgramStore {
    /// Create `root` if needed.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, ApiError> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(|e| ApiError::internal(e.to_string()))?;
        Ok(Self { root })
    }

    /// Store root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Parse framing (not signature) and write `<id>/`.
    pub fn put(
        &self,
        bytes: &[u8],
        uploader: Option<&str>,
        stored_at: u64,
    ) -> Result<StoredMeta, ApiError> {
        let parsed = parse_spkg(bytes)?;
        validate_program_id(&parsed.manifest.id)
            .map_err(|e| ApiError::bad_request("validation", e.to_string()))?;
        let id = parsed.manifest.id.clone();
        let dir = self.root.join(&id);
        fs::create_dir_all(&dir).map_err(|e| ApiError::internal(e.to_string()))?;
        let pkg_path = dir.join("package.spkg");
        let tmp = dir.join("package.spkg.tmp");
        fs::write(&tmp, bytes).map_err(|e| ApiError::internal(e.to_string()))?;
        fs::rename(&tmp, &pkg_path).map_err(|e| ApiError::internal(e.to_string()))?;
        let meta = StoredMeta {
            id,
            version: parsed.manifest.version,
            build_id: parsed.manifest.build_id,
            compatibility_hash: parsed.manifest.compatibility_hash,
            size: bytes.len() as u64,
            uploader: uploader.map(str::to_string),
            stored_at,
        };
        let meta_json =
            serde_json::to_vec_pretty(&meta).map_err(|e| ApiError::internal(e.to_string()))?;
        fs::write(dir.join("meta.json"), meta_json)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        Ok(meta)
    }

    /// Load metadata + bytes.
    pub fn get(&self, id: &str) -> Result<(StoredMeta, Vec<u8>), ApiError> {
        validate_program_id(id).map_err(|_| ApiError::not_found(id))?;
        let dir = self.root.join(id);
        if !dir.is_dir() {
            return Err(ApiError::not_found(format!("program '{id}'")));
        }
        let meta = load_meta(&dir.join("meta.json"))?;
        let bytes = fs::read(dir.join("package.spkg"))
            .map_err(|_| ApiError::not_found(format!("program '{id}'")))?;
        Ok((meta, bytes))
    }

    /// List stored programs (not pointer files).
    pub fn list(&self) -> Result<Vec<StoredMeta>, ApiError> {
        let mut out = Vec::new();
        let rd = fs::read_dir(&self.root).map_err(|e| ApiError::internal(e.to_string()))?;
        for ent in rd {
            let ent = ent.map_err(|e| ApiError::internal(e.to_string()))?;
            if !ent.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let meta_path = ent.path().join("meta.json");
            if meta_path.is_file() {
                out.push(load_meta(&meta_path)?);
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    /// Remove an inactive program directory.
    pub fn delete(&self, id: &str) -> Result<(), ApiError> {
        validate_program_id(id).map_err(|_| ApiError::not_found(id))?;
        let dir = self.root.join(id);
        if !dir.is_dir() {
            return Err(ApiError::not_found(format!("program '{id}'")));
        }
        fs::remove_dir_all(dir).map_err(|e| ApiError::internal(e.to_string()))?;
        Ok(())
    }

    /// Write pointer file `current` or `armed` (empty removes).
    pub fn set_pointer(&self, name: &str, id: Option<&str>) -> Result<(), ApiError> {
        let path = self.root.join(name);
        match id {
            Some(id) => fs::write(path, id).map_err(|e| ApiError::internal(e.to_string()))?,
            None => {
                if path.exists() {
                    fs::remove_file(path).map_err(|e| ApiError::internal(e.to_string()))?;
                }
            }
        }
        Ok(())
    }
}

fn load_meta(path: &Path) -> Result<StoredMeta, ApiError> {
    let text = fs::read_to_string(path).map_err(|e| ApiError::internal(e.to_string()))?;
    serde_json::from_str(&text).map_err(|e| ApiError::internal(e.to_string()))
}
