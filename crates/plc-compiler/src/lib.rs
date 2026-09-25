//! Host compiler: Appendix B ST-subset → IR v0.1 → `.spkg`.
//!
//! Non-RT crate. Controllers never parse ST (KD-3).

#![forbid(unsafe_code)]
#![allow(
    clippy::assigning_clones,
    clippy::collapsible_if,
    clippy::explicit_counter_loop,
    clippy::match_wildcard_for_single_variants,
    clippy::needless_lifetimes,
    clippy::needless_pass_by_value,
    clippy::semicolon_if_nothing_returned,
    clippy::unnested_or_patterns,
    clippy::unnecessary_wraps
)]

mod ast;
mod bind;
mod error;
mod lex;
pub mod pack;
mod parse;
mod project;

pub use error::{CompileError, ErrorCode, Span};
pub use pack::{build_spkg, emit_spasm_listing, write_file};
pub use parse::parse;
pub use project::{load_project, LoadedProject, ProjectFile};

use std::path::Path;

use plc_ir::IrModule;
use plc_package::Manifest;

/// Options for [`compile_project`].
#[derive(Debug, Clone, Default)]
pub struct CompileOptions {
    /// Override `build_id` from project.toml.
    pub build_id: Option<String>,
    /// Optional Ed25519 seed hex (64 chars) for signing.
    pub signing_seed_hex: Option<String>,
    /// When true, populate [`CompileOutput::spasm`].
    pub emit_spasm: bool,
}

/// Successful compile result.
#[derive(Debug, Clone)]
pub struct CompileOutput {
    /// Verified IR module.
    pub module: IrModule,
    /// Package manifest (hash filled).
    pub manifest: Manifest,
    /// `.spkg` bytes.
    pub spkg: Vec<u8>,
    /// Optional spasm-like listing.
    pub spasm: Option<String>,
}

/// Compile a `project.toml` path into a closed package.
pub fn compile_project(
    project_path: &Path,
    opts: &CompileOptions,
) -> Result<CompileOutput, CompileError> {
    let loaded = load_project(project_path)?;
    let build_id = opts
        .build_id
        .clone()
        .or_else(|| loaded.file.build_id.clone())
        .unwrap_or_else(|| "dev".into());
    let (module, manifest) = bind::compile_loaded(&loaded, &build_id)?;
    let spkg = build_spkg(&module, manifest.clone(), opts.signing_seed_hex.as_deref())?;
    let spasm = if opts.emit_spasm {
        Some(emit_spasm_listing(&module))
    } else {
        None
    };
    Ok(CompileOutput {
        module,
        manifest,
        spkg,
        spasm,
    })
}

/// Parse-only helper for tests.
pub fn parse_st(source: &str) -> Result<ast::CompilationUnit, CompileError> {
    parse(source)
}
