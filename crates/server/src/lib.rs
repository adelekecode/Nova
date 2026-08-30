//! TCP server runtime for Nova.

use std::{future::Future, io::IsTerminal, path::Path, sync::Arc};

use anyhow::Context;
use nova_engine::Engine;
use nova_protocol::{Command, parse};
use nova_types::Point;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::Mutex,
};
use tracing::{info, warn};

const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Short git commit hash captured by `build.rs`, or `"unknown"` outside a git checkout.
const GIT_SHA: &str = env!("NOVA_BUILD_GIT_SHA");
/// `rustc` version used to compile this binary, captured by `build.rs`.
const RUSTC_VERSION: &str = env!("NOVA_BUILD_RUSTC_VERSION");
/// `"debug"` or `"release"`, based on the active Cargo profile.
const PROFILE: &str = if cfg!(debug_assertions) {
    "debug"
} else {
    "release"
};

/// Shared engine handle used by the server's connection tasks.
pub type SharedEngine = Arc<Mutex<Engine>>;

/// Prints a Redis-style startup banner to the terminal.
///
/// Padding is applied to the plain text first and only then wrapped in ANSI color codes, since
/// escape sequences count toward a naive `{:<width}` fill and would otherwise throw off column
/// alignment whenever color is enabled.
pub fn print_banner(listen: &str, data_dir: &Path) {
    let color = std::io::stdout().is_terminal();
    let colorize = |code: &str, text: &str| -> String {
        if color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    };

    let art_lines: [&str; 9] = [
        "",
        "",
        r"        \   |   /",
        r"          \ | /",
        r"  ---------(*)---------",
        r"          / | \",
        r"        /   |   \",
        "",
        "",
    ];
    let info_lines = [
        format!("Nova {VERSION}"),
        "A Redis-inspired time-series database".to_owned(),
        String::new(),
        format!("Port       {listen}"),
        format!("PID        {}", std::process::id()),
        format!("Data dir   {}", data_dir.display()),
        format!("Build      {GIT_SHA} ({PROFILE})"),
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

/// Accepts TCP connections until the supplied shutdown future resolves.
///
/// # Errors
///
/// Returns an error if accepting a connection fails or the shutdown future reports an error.
pub async fn serve_until_shutdown(
    listener: TcpListener,
    engine: SharedEngine,
    shutdown: impl Future<Output = anyhow::Result<()>>,
) -> anyhow::Result<()> {
    let shutdown = shutdown;
    tokio::pin!(shutdown);

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
            signal = &mut shutdown => {
                signal?;
                info!("Nova is shutting down");
                return Ok(());
            }
        }
    }
}

async fn handle_connection(stream: TcpStream, engine: SharedEngine) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        let response = execute(&line, &engine).await;
        writer.write_all(response.as_bytes()).await?;
        writer.write_all(b"\n").await?;
    }
    Ok(())
}

/// Formats a wire error response as `ERR <CODE> <message>`, where `code` is a stable,
/// machine-readable identifier (see each crate's `code()` method) and `message` is the
/// human-readable [`std::fmt::Display`] text.
fn err(code: &str, message: impl std::fmt::Display) -> String {
    format!("ERR {code} {message}")
}

async fn execute(input: &str, engine: &Mutex<Engine>) -> String {
    let command = match parse(input) {
        Ok(command) => command,
        Err(error) => return err(error.code(), error),
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
            Err(error) => err(error.code(), error),
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
                Err(error) => err(error.code(), error),
            }
        }
        Command::Info => {
            let engine = engine.lock().await;
            format!(
                "INFO version={VERSION} git={GIT_SHA} rustc={RUSTC_VERSION} profile={PROFILE} metrics={} points={}",
                engine.metric_count(),
                engine.point_count()
            )
        }
    }
}
