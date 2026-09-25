//! `plc-compiler` CLI.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use plc_compiler::{compile_project, write_file, CompileOptions};

#[derive(Debug, Parser)]
#[command(
    name = "plc-compiler",
    version,
    about = "Compile Appendix B ST → .spkg"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Compile a project.toml to a .spkg package.
    Compile {
        /// Path to project.toml.
        project: PathBuf,
        /// Output .spkg path.
        #[arg(short, long)]
        output: PathBuf,
        /// Optional Ed25519 seed hex file (64 hex chars).
        #[arg(long)]
        sign: Option<PathBuf>,
        /// Write a spasm-like listing to this path.
        #[arg(long)]
        emit_spasm: Option<PathBuf>,
        /// Override build_id.
        #[arg(long)]
        build_id: Option<String>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Compile {
            project,
            output,
            sign,
            emit_spasm,
            build_id,
        } => {
            let signing_seed_hex = match sign {
                Some(p) => match plc_compiler::pack::load_seed_file(&p) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        eprintln!("{e}");
                        return ExitCode::FAILURE;
                    }
                },
                None => None,
            };
            let opts = CompileOptions {
                build_id,
                signing_seed_hex,
                emit_spasm: emit_spasm.is_some(),
            };
            match compile_project(&project, &opts) {
                Ok(out) => {
                    if let Err(e) = write_file(&output, &out.spkg) {
                        eprintln!("{e}");
                        return ExitCode::FAILURE;
                    }
                    if let (Some(path), Some(text)) = (emit_spasm, out.spasm) {
                        if let Err(e) = write_file(&path, text.as_bytes()) {
                            eprintln!("{e}");
                            return ExitCode::FAILURE;
                        }
                    }
                    println!(
                        "wrote {} ({} bytes, id={}, version={})",
                        output.display(),
                        out.spkg.len(),
                        out.manifest.id,
                        out.manifest.version
                    );
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}
