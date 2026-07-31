use std::{path::PathBuf, sync::Arc};

use anyhow::Context;
use clap::Parser;
use nova_engine::Engine;
use nova_protocol::{Command, parse};
use nova_types::Point;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::Mutex,
};
use tracing::{info, warn};
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
    info!(address = %arguments.listen, data_dir = %arguments.data_dir.display(), "Nova is ready");

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, peer) = accepted.context("failed to accept connection")?;
                let engine = Arc::clone(&engine);
                tokio::spawn(async move {
                    if let Err(error) = handle_connection(stream, engine).await {
                        warn!(%peer, %error, "connection failed");
                    }
                });
            }
            signal = tokio::signal::ctrl_c() => {
                signal.context("failed to listen for shutdown signal")?;
                info!("Nova is shutting down");
                return Ok(());
            }
        }
    }
}

async fn handle_connection(stream: TcpStream, engine: Arc<Mutex<Engine>>) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        let response = execute(&line, &engine).await;
        writer.write_all(response.as_bytes()).await?;
        writer.write_all(b"\n").await?;
    }
    Ok(())
}

async fn execute(input: &str, engine: &Mutex<Engine>) -> String {
    let command = match parse(input) {
        Ok(command) => command,
        Err(error) => return format!("ERR {error}"),
    };

    match command {
        Command::Ping => "PONG".to_owned(),
        Command::Write {
            metric,
            timestamp,
            value,
        } => match engine
            .lock()
            .await
            .write(metric, &Point::new(timestamp, value))
        {
            Ok(()) => "OK".to_owned(),
            Err(error) => format!("ERR {error}"),
        },
        Command::Range { metric, start, end } => {
            match engine.lock().await.range(&metric, start, end) {
                Ok(points) => {
                    let body = points
                        .iter()
                        .map(|point| format!("{} {}", point.timestamp, point.value))
                        .collect::<Vec<_>>()
                        .join(";");
                    format!("POINTS {} {body}", points.len())
                }
                Err(error) => format!("ERR {error}"),
            }
        }
        Command::Info => {
            let engine = engine.lock().await;
            format!(
                "INFO metrics={} points={}",
                engine.metric_count(),
                engine.point_count()
            )
        }
    }
}
