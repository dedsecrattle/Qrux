use anyhow::Result;
use clap::Parser;
use qrux::{config, server};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "qrux", version, about = "Qrux — QUIC/HTTP3 terminating proxy")]
struct Args {
    /// Path to the configuration file
    #[arg(short, long, default_value = "proxy.toml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Install the default crypto provider (aws-lc-rs)
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install crypto provider");

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("qrux=info".parse()?))
        .init();

    let args = Args::parse();
    let config = config::Config::load(&args.config)?;

    tracing::info!(listen = %config.server.listen, "Starting qrux");

    server::run(config).await
}
