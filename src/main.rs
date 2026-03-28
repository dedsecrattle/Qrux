use anyhow::Result;
use quicproxy::{config, server};
use clap::Parser;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "quicproxy", version, about = "QUIC/HTTP3 terminating proxy")]
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
        .with_env_filter(EnvFilter::from_default_env().add_directive("quicproxy=info".parse()?))
        .init();

    let args = Args::parse();
    let config = config::Config::load(&args.config)?;

    tracing::info!(listen = %config.server.listen, "Starting quicproxy");

    server::run(config).await
}
