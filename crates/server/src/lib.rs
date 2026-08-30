//! TCP server runtime for Nova.

use std::{
    future::Future,
    io::{self, ErrorKind, IsTerminal},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, anyhow};
use nova_engine::Engine;
use nova_protocol::{Command, parse};
use nova_types::Point;
use serde::Deserialize;
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::{Mutex, watch},
    task::JoinSet,
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

/// Maximum accepted request line length, in bytes.
pub const MAX_REQUEST_BYTES: usize = 8 * 1024;
/// Maximum number of concurrently active TCP connections.
pub const MAX_CONNECTIONS: usize = 1024;

/// Runtime limits for the TCP server.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServerLimits {
    /// Maximum number of concurrently active TCP connections.
    pub max_connections: usize,
    /// Maximum accepted request line length, in bytes.
    pub max_request_bytes: usize,
}

/// Complete runtime configuration for `nova-server`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerConfig {
    /// Address on which Nova accepts TCP connections.
    pub listen: String,
    /// Directory containing durable Nova data.
    pub data_dir: PathBuf,
    /// Whether to suppress the startup banner.
    pub no_banner: bool,
    /// TCP runtime limits.
    pub limits: ServerLimits,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:7422".to_owned(),
            data_dir: PathBuf::from("./nova-data"),
            no_banner: false,
            limits: ServerLimits::default(),
        }
    }
}

/// Optional configuration overrides from CLI arguments or environment variables.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ServerConfigOverrides {
    /// Override for [`ServerConfig::listen`].
    pub listen: Option<String>,
    /// Override for [`ServerConfig::data_dir`].
    pub data_dir: Option<PathBuf>,
    /// Override for [`ServerConfig::no_banner`].
    pub no_banner: Option<bool>,
    /// Override for [`ServerLimits::max_connections`].
    pub max_connections: Option<usize>,
    /// Override for [`ServerLimits::max_request_bytes`].
    pub max_request_bytes: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    listen: Option<String>,
    data_dir: Option<PathBuf>,
    no_banner: Option<bool>,
    max_connections: Option<usize>,
    max_request_bytes: Option<usize>,
}

impl From<ConfigFile> for ServerConfigOverrides {
    fn from(value: ConfigFile) -> Self {
        Self {
            listen: value.listen,
            data_dir: value.data_dir,
            no_banner: value.no_banner,
            max_connections: value.max_connections,
            max_request_bytes: value.max_request_bytes,
        }
    }
}

impl Default for ServerLimits {
    fn default() -> Self {
        Self {
            max_connections: MAX_CONNECTIONS,
            max_request_bytes: MAX_REQUEST_BYTES,
        }
    }
}

/// Loads server configuration using `defaults < config file < environment < CLI` precedence.
///
/// # Errors
///
/// Returns an error if the config file cannot be read or parsed, or if an environment override has
/// an invalid value.
pub fn load_server_config(
    config_path: Option<&Path>,
    cli: ServerConfigOverrides,
) -> anyhow::Result<ServerConfig> {
    let file = config_path
        .map(read_config_file)
        .transpose()?
        .unwrap_or_default();
    let env = server_config_overrides_from_env()?;
    Ok(resolve_server_config(file.into(), env, cli))
}

fn read_config_file(path: &Path) -> anyhow::Result<ConfigFile> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    toml::from_str(&contents)
        .with_context(|| format!("failed to parse config file {}", path.display()))
}

fn server_config_overrides_from_env() -> anyhow::Result<ServerConfigOverrides> {
    Ok(ServerConfigOverrides {
        listen: std::env::var("NOVA_LISTEN").ok(),
        data_dir: std::env::var_os("NOVA_DATA_DIR").map(PathBuf::from),
        no_banner: parse_optional_bool_env("NOVA_NO_BANNER")?,
        max_connections: parse_optional_usize_env("NOVA_MAX_CONNECTIONS")?,
        max_request_bytes: parse_optional_usize_env("NOVA_MAX_REQUEST_BYTES")?,
    })
}

fn parse_optional_usize_env(name: &str) -> anyhow::Result<Option<usize>> {
    std::env::var(name)
        .ok()
        .map(|value| {
            value
                .parse()
                .with_context(|| format!("{name} must be a non-negative integer"))
        })
        .transpose()
}

fn parse_optional_bool_env(name: &str) -> anyhow::Result<Option<bool>> {
    std::env::var(name)
        .ok()
        .map(|value| parse_bool(name, &value))
        .transpose()
}

fn parse_bool(name: &str, value: &str) -> anyhow::Result<bool> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(anyhow!(
            "{name} must be one of true, false, 1, 0, yes, no, on, or off"
        )),
    }
}

fn resolve_server_config(
    file: ServerConfigOverrides,
    env: ServerConfigOverrides,
    cli: ServerConfigOverrides,
) -> ServerConfig {
    let mut config = ServerConfig::default();
    apply_overrides(&mut config, file);
    apply_overrides(&mut config, env);
    apply_overrides(&mut config, cli);
    config
}

fn apply_overrides(config: &mut ServerConfig, overrides: ServerConfigOverrides) {
    if let Some(listen) = overrides.listen {
        config.listen = listen;
    }
    if let Some(data_dir) = overrides.data_dir {
        config.data_dir = data_dir;
    }
    if let Some(no_banner) = overrides.no_banner {
        config.no_banner = no_banner;
    }
    if let Some(max_connections) = overrides.max_connections {
        config.limits.max_connections = max_connections;
    }
    if let Some(max_request_bytes) = overrides.max_request_bytes {
        config.limits.max_request_bytes = max_request_bytes;
    }
}

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
    serve_with_limits_until_shutdown(listener, engine, ServerLimits::default(), shutdown).await
}

/// Accepts TCP connections with explicit runtime limits until the supplied shutdown future
/// resolves.
///
/// # Errors
///
/// Returns an error if accepting a connection fails or the shutdown future reports an error.
pub async fn serve_with_limits_until_shutdown(
    listener: TcpListener,
    engine: SharedEngine,
    limits: ServerLimits,
    shutdown: impl Future<Output = anyhow::Result<()>>,
) -> anyhow::Result<()> {
    let shutdown = shutdown;
    tokio::pin!(shutdown);
    let (shutdown_sender, shutdown_receiver) = watch::channel(false);
    let mut connections = JoinSet::new();

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (mut stream, peer) = accepted.context("failed to accept connection")?;
                if connections.len() >= limits.max_connections {
                    if let Err(error) = reject_connection(&mut stream).await {
                        warn!(%peer, %error, "failed to reject connection");
                    }
                    continue;
                }
                let engine = Arc::clone(&engine);
                let shutdown_receiver = shutdown_receiver.clone();
                let max_request_bytes = limits.max_request_bytes;
                connections.spawn(async move {
                    (
                        peer,
                        handle_connection(stream, engine, shutdown_receiver, max_request_bytes).await,
                    )
                });
            }
            Some(connection) = connections.join_next(), if !connections.is_empty() => {
                log_connection_result(connection);
            }
            signal = &mut shutdown => {
                signal?;
                info!("Nova is shutting down");
                break;
            }
        }
    }

    let _ = shutdown_sender.send(true);
    while let Some(connection) = connections.join_next().await {
        log_connection_result(connection);
    }

    engine
        .lock()
        .await
        .flush()
        .context("failed to flush Nova engine during shutdown")?;
    info!("Nova shutdown complete");
    Ok(())
}

async fn reject_connection(stream: &mut TcpStream) -> anyhow::Result<()> {
    stream
        .write_all(b"ERR TOO_MANY_CONNECTIONS maximum connection limit reached\n")
        .await?;
    stream.shutdown().await?;
    Ok(())
}

fn log_connection_result(
    connection: Result<(std::net::SocketAddr, anyhow::Result<()>), tokio::task::JoinError>,
) {
    match connection {
        Ok((_, Ok(()))) => {}
        Ok((peer, Err(error))) => warn!(%peer, %error, "connection failed"),
        Err(error) => warn!(%error, "connection task failed"),
    }
}

async fn handle_connection(
    stream: TcpStream,
    engine: SharedEngine,
    mut shutdown: watch::Receiver<bool>,
    max_request_bytes: usize,
) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    loop {
        tokio::select! {
            line = read_line_limited(&mut reader, max_request_bytes) => {
                match line? {
                    ReadLine::Line(line) => {
                        let response = execute(&line, &engine).await;
                        writer.write_all(response.as_bytes()).await?;
                        writer.write_all(b"\n").await?;
                    }
                    ReadLine::TooLarge => {
                        writer
                            .write_all(
                                format!(
                                    "ERR REQUEST_TOO_LARGE request exceeds maximum of {max_request_bytes} bytes\n"
                                )
                                .as_bytes(),
                            )
                            .await?;
                        writer.shutdown().await?;
                        return Ok(());
                    }
                    ReadLine::Eof => return Ok(()),
                }
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    writer.shutdown().await?;
                    return Ok(());
                }
            }
        }
    }
}

enum ReadLine {
    Line(String),
    Eof,
    TooLarge,
}

async fn read_line_limited(
    reader: &mut (impl AsyncBufRead + Unpin),
    max_bytes: usize,
) -> io::Result<ReadLine> {
    let mut line = Vec::new();

    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            if line.is_empty() {
                return Ok(ReadLine::Eof);
            }
            return decode_line(line);
        }

        let take = available
            .iter()
            .position(|&byte| byte == b'\n')
            .map_or(available.len(), |position| position + 1);

        if line.len() + take > max_bytes {
            return Ok(ReadLine::TooLarge);
        }

        let ends_line = available[..take].ends_with(b"\n");
        line.extend_from_slice(&available[..take]);
        reader.consume(take);

        if ends_line {
            if line.ends_with(b"\n") {
                line.pop();
            }
            if line.ends_with(b"\r") {
                line.pop();
            }
            return decode_line(line);
        }
    }
}

fn decode_line(line: Vec<u8>) -> io::Result<ReadLine> {
    String::from_utf8(line)
        .map(ReadLine::Line)
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))
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
        Command::Batch { writes } => {
            let records = writes
                .into_iter()
                .map(|write| (write.metric, Point::new(write.timestamp, write.value)));
            match engine.lock().await.write_batch(records) {
                Ok(()) => "OK".to_owned(),
                Err(error) => err(error.code(), error),
            }
        }
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        ServerConfig, ServerConfigOverrides, ServerLimits, read_config_file, resolve_server_config,
    };

    #[test]
    fn resolves_config_with_documented_precedence() {
        let config = resolve_server_config(
            ServerConfigOverrides {
                listen: Some("127.0.0.1:7000".to_owned()),
                data_dir: Some(PathBuf::from("/config")),
                no_banner: Some(true),
                max_connections: Some(10),
                max_request_bytes: Some(100),
            },
            ServerConfigOverrides {
                listen: Some("127.0.0.1:8000".to_owned()),
                data_dir: Some(PathBuf::from("/env")),
                no_banner: Some(false),
                max_connections: Some(20),
                max_request_bytes: Some(200),
            },
            ServerConfigOverrides {
                listen: Some("127.0.0.1:9000".to_owned()),
                data_dir: None,
                no_banner: Some(true),
                max_connections: None,
                max_request_bytes: Some(300),
            },
        );

        assert_eq!(
            config,
            ServerConfig {
                listen: "127.0.0.1:9000".to_owned(),
                data_dir: PathBuf::from("/env"),
                no_banner: true,
                limits: ServerLimits {
                    max_connections: 20,
                    max_request_bytes: 300,
                },
            }
        );
    }

    #[test]
    fn reads_toml_config_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nova.toml");
        std::fs::write(
            &path,
            r#"
listen = "127.0.0.1:7777"
data_dir = "/tmp/nova-test"
no_banner = true
max_connections = 64
max_request_bytes = 4096
"#,
        )
        .unwrap();

        let file = read_config_file(&path).unwrap();
        let config = resolve_server_config(
            file.into(),
            ServerConfigOverrides::default(),
            ServerConfigOverrides::default(),
        );

        assert_eq!(config.listen, "127.0.0.1:7777");
        assert_eq!(config.data_dir, PathBuf::from("/tmp/nova-test"));
        assert!(config.no_banner);
        assert_eq!(config.limits.max_connections, 64);
        assert_eq!(config.limits.max_request_bytes, 4096);
    }
}
