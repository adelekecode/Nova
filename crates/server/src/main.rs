use std::{
    io::IsTerminal,
    path::{Path, PathBuf},
    sync::Arc,
};

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

const VERSION: &str = env!("CARGO_PKG_VERSION");

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

/// Prints a Redis-style startup banner to the terminal.
///
/// Padding is applied to the plain text first and only then wrapped in ANSI color codes, since
/// escape sequences count toward a naive `{:<width}` fill and would otherwise throw off column
/// alignment whenever color is enabled.
fn print_banner(listen: &str, data_dir: &Path) {
    let color = std::io::stdout().is_terminal();
    let colorize = |code: &str, text: &str| -> String {
        if color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    };

    let art_lines: [&str; 8] = [
        "",
        "",
        r"        \   |   /",
        r"          \ | /",
        r"  ---------(*)---------",
        r"          / | \",
        r"        /   |   \",
        "",
    ];
    let info_lines = [
        format!("Nova {VERSION}"),
        "A Redis-inspired time-series database".to_owned(),
        String::new(),
        format!("Port       {listen}"),
        format!("PID        {}", std::process::id()),
        format!("Data dir   {}", data_dir.display()),
        "Mode       standalone".to_owned(),
        String::new(),
    ];

    println!();
    for (index, (left, right)) in art_lines.iter().zip(info_lines.iter()).enumerate() {
        let padded_left = format!("{left:<34}");
        let styled_left = colorize("36", &padded_left);
        let styled_right = if index == 0 {
            colorize("1", right)
        } else {
            right.clone()
        };
        println!("{styled_left}{styled_right}");
    }
    println!("Ready to accept connections");
    println!();
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
