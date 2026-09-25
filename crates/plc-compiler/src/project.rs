//! `project.toml` / `lib.toml` loading.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{CompileError, ErrorCode};

/// Root compile project.
#[derive(Debug, Clone, Deserialize)]
pub struct ProjectFile {
    /// Program id.
    pub id: String,
    /// Semver.
    pub version: String,
    /// Build id (optional; CLI may override).
    #[serde(default)]
    pub build_id: Option<String>,
    /// Restart policy string.
    #[serde(default = "default_restart")]
    pub restart_policy: String,
    /// Library paths relative to project file.
    #[serde(default)]
    pub libs: Vec<String>,
    /// ST source paths relative to project file.
    pub sources: Vec<String>,
    /// Task → PROGRAM mapping.
    #[serde(default)]
    pub task: Vec<TaskMap>,
    /// Tag dictionary entries.
    #[serde(default)]
    pub tag: Vec<TagMap>,
    /// Max primitive instances per kind (default 4096).
    #[serde(default = "default_max_instances")]
    pub max_instances: u32,
}

fn default_restart() -> String {
    "safe_reset".into()
}

fn default_max_instances() -> u32 {
    4096
}

/// Task mapping entry.
#[derive(Debug, Clone, Deserialize)]
pub struct TaskMap {
    /// Config / manifest task name (`fast`).
    pub name: String,
    /// ST `PROGRAM` name (`Fast`).
    pub program: String,
}

/// Tag dictionary / binding entry.
#[derive(Debug, Clone, Deserialize)]
pub struct TagMap {
    /// Tag path.
    pub name: String,
    /// `I` / `Q` / `M` / `R`.
    pub kind: String,
    /// IEC type name.
    #[serde(rename = "type")]
    pub ty: String,
    /// Slot for I/Q.
    #[serde(default)]
    pub slot: Option<u32>,
    /// Offset for R (and optional M).
    #[serde(default)]
    pub offset: Option<u32>,
}

/// Library manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct LibFile {
    /// Library name.
    pub name: String,
    /// Semver.
    #[allow(dead_code)]
    pub version: String,
    /// Source globs / paths relative to lib.toml.
    pub sources: Vec<String>,
}

/// Loaded project with resolved paths.
#[derive(Debug, Clone)]
pub struct LoadedProject {
    /// Directory containing project.toml.
    pub root: PathBuf,
    /// Parsed project.
    pub file: ProjectFile,
    /// Absolute ST sources (app).
    pub sources: Vec<PathBuf>,
    /// Library ST sources in link order.
    pub lib_sources: Vec<(String, PathBuf)>,
}

/// Load `project.toml` and resolve source paths.
pub fn load_project(path: &Path) -> Result<LoadedProject, CompileError> {
    let path = path
        .canonicalize()
        .map_err(|e| CompileError::new(ErrorCode::EProject, format!("project path: {e}")))?;
    let root = path
        .parent()
        .ok_or_else(|| CompileError::new(ErrorCode::EProject, "project has no parent dir"))?
        .to_path_buf();
    let text = std::fs::read_to_string(&path)
        .map_err(|e| CompileError::new(ErrorCode::EProject, format!("read project: {e}")))?;
    let file: ProjectFile = toml::from_str(&text)
        .map_err(|e| CompileError::new(ErrorCode::EProject, format!("parse project.toml: {e}")))?;
    if file.sources.is_empty() {
        return Err(CompileError::new(
            ErrorCode::EProject,
            "project.sources must not be empty",
        ));
    }
    if file.task.is_empty() {
        return Err(CompileError::new(
            ErrorCode::EProject,
            "project must declare at least one [[task]]",
        ));
    }
    let mut sources = Vec::new();
    for s in &file.sources {
        let p = root.join(s);
        if !p.is_file() {
            return Err(CompileError::new(
                ErrorCode::EProject,
                format!("source not found: {}", p.display()),
            ));
        }
        sources.push(p);
    }
    let mut lib_sources = Vec::new();
    for lib_rel in &file.libs {
        let lib_root = root.join(lib_rel);
        let lib_toml = if lib_root.is_file() {
            lib_root.clone()
        } else {
            lib_root.join("lib.toml")
        };
        let lib_dir = lib_toml
            .parent()
            .ok_or_else(|| CompileError::new(ErrorCode::EProject, "lib.toml parent"))?
            .to_path_buf();
        let text = std::fs::read_to_string(&lib_toml).map_err(|e| {
            CompileError::new(
                ErrorCode::EProject,
                format!("read {}: {e}", lib_toml.display()),
            )
        })?;
        let lib: LibFile = toml::from_str(&text).map_err(|e| {
            CompileError::new(
                ErrorCode::EProject,
                format!("parse {}: {e}", lib_toml.display()),
            )
        })?;
        for s in &lib.sources {
            let p = lib_dir.join(s);
            if !p.is_file() {
                return Err(CompileError::new(
                    ErrorCode::EProject,
                    format!("lib source not found: {}", p.display()),
                ));
            }
            lib_sources.push((lib.name.clone(), p));
        }
    }
    Ok(LoadedProject {
        root,
        file,
        sources,
        lib_sources,
    })
}
