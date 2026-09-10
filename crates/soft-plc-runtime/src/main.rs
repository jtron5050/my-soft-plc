//! `soft-plc-runtime` process entry.

use clap::Parser;
use soft_plc_runtime::{apply_cli, load_config, Args, Supervisor};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    if let Err(e) = run().await {
        tracing::error!("{e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), soft_plc_runtime::AppError> {
    let args = Args::parse();
    let mut cfg = load_config(&args.config)?;
    apply_cli(&mut cfg, args.data_dir.as_ref(), args.bind.as_deref());
    let config_path = Some(args.config.clone());
    let sup = Supervisor::boot(cfg, config_path, args.program, args.mode.as_deref()).await?;
    tracing::info!("soft-plc-runtime listening on {}", sup.listen);
    sup.serve().await
}
