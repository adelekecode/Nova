use std::{path::PathBuf, sync::Arc};

use anyhow::Context;
use clap::Parser;
use nova_engine::Engine;
use nova_server::{
    ServerConfigOverrides, load_server_config, print_banner, serve_with_limits_until_shutdown,
};
use tokio::{net::TcpListener, sync::Mutex};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "nova-server", version, about = "Nova time-series database")]
struct Arguments {
    /// Path to a TOML configuration file.
    #[arg(long, env = "NOVA_CONFIG")]
    config: Option<PathBuf>,
    /// Address on which Nova accepts TCP connections.
    #[arg(long)]
    listen: Option<String>,
    /// Directory containing durable Nova data.
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// Maximum number of concurrently active TCP connections.
    #[arg(long)]
    max_connections: Option<usize>,
    /// Maximum accepted request line length, in bytes.
    #[arg(long)]
    max_request_bytes: Option<usize>,
    /// Maximum time a client can stay connected without completing a request line, in milliseconds.
    #[arg(long)]
    client_idle_timeout_ms: Option<u64>,
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
    let config = load_server_config(
        arguments.config.as_deref(),
        ServerConfigOverrides {
            listen: arguments.listen,
            data_dir: arguments.data_dir,
            no_banner: arguments.no_banner.then_some(true),
            max_connections: arguments.max_connections,
            max_request_bytes: arguments.max_request_bytes,
            client_idle_timeout_ms: arguments.client_idle_timeout_ms,
        },
    )?;
    let engine = Arc::new(Mutex::new(
        Engine::open(&config.data_dir).context("failed to open Nova engine")?,
    ));
    let listener = TcpListener::bind(&config.listen)
        .await
        .with_context(|| format!("failed to bind {}", config.listen))?;

    if !config.no_banner {
        print_banner(&config.listen, &config.data_dir);
    }
    info!(address = %config.listen, data_dir = %config.data_dir.display(), "Nova is ready");

    serve_with_limits_until_shutdown(listener, engine, config.limits, async {
        tokio::signal::ctrl_c()
            .await
            .context("failed to listen for shutdown signal")
    })
    .await
}
