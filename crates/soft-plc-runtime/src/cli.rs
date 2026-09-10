//! Command-line options.

use std::path::PathBuf;

use clap::Parser;

/// Soft PLC runtime process (`soft-plc-runtime`).
#[derive(Debug, Parser)]
#[command(
    name = "soft-plc-runtime",
    version,
    about = "Soft PLC runtime (SIM demo / device)"
)]
pub struct Args {
    /// Device YAML/JSON (`samples/configs/sim-plant.yaml`).
    #[arg(long)]
    pub config: PathBuf,
    /// Remap `paths.programs` / `retain` / `audit` under this directory.
    #[arg(long)]
    pub data_dir: Option<PathBuf>,
    /// Load, arm, and activate this `.spkg` on boot (demo).
    #[arg(long)]
    pub program: Option<PathBuf>,
    /// After a program is current: `STOP`, `SIM`, or `RUN` (default STOP).
    #[arg(long)]
    pub mode: Option<String>,
    /// Override `rest.bind` (`host:port`).
    #[arg(long)]
    pub bind: Option<String>,
}
