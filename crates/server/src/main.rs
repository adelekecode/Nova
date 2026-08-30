use std::{path::PathBuf, sync::Arc};

use anyhow::Context;
use clap::Parser;
use nova_engine::Engine;
use nova_server::{print_banner, serve_until_shutdown};
use tokio::{net::TcpListener, sync::Mutex};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "nova-server", version, about = "Nova time-series database")]
struct Arguments {
    /// Address on which Nova accepts TCP connections.
    #[arg(long, default_value = "127.0.0.1:7422", env = "NOVA_LISTEN")]
    listen: String,
    /// Directory containing durable Nova data.
    #[arg(long, default_value = "./nova-data", env = "NOVA_DATA_DIR")]
    data_dir: PathBuf,
    /// Suppress the startup banner.
    #[arg(long)]
    no_banner: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "nova=info".into()))
        .init();

    let arguments = Arguments::parse();
    let engine = Arc::new(Mutex::new(
        Engine::open(&arguments.data_dir).context("failed to open Nova engine")?,
    ));
    let listener = TcpListener::bind(&arguments.listen)
        .await
        .with_context(|| format!("failed to bind {}", arguments.listen))?;

    if !arguments.no_banner {
        print_banner(&arguments.listen, &arguments.data_dir);
    }
    info!(address = %arguments.listen, data_dir = %arguments.data_dir.display(), "Nova is ready");

    serve_until_shutdown(listener, engine, async {
        tokio::signal::ctrl_c()
            .await
            .context("failed to listen for shutdown signal")
    })
    .await
}
