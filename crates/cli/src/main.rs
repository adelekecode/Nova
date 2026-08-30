use std::{
    env,
    fmt::Write as _,
    io::{self, IsTerminal, Write},
    path::PathBuf,
};

use anyhow::Context;
use clap::{ArgAction, Parser};
use rustyline::{DefaultEditor, error::ReadlineError};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{
        TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 7422;

#[derive(Debug, Parser)]
#[command(
    name = "nova-cli",
    version,
    about = "Command-line client for Nova",
    disable_help_flag = true
)]
struct Arguments {
    /// Print help.
    #[arg(long, action = ArgAction::Help)]
    help: Option<bool>,
    /// Full Nova server address. Overrides --host and --port.
    #[arg(long, env = "NOVA_ADDRESS")]
    address: Option<String>,
    /// Nova server host.
    #[arg(short = 'h', long, default_value = DEFAULT_HOST, env = "NOVA_HOST")]
    host: String,
    /// Nova server port.
    #[arg(short = 'p', long, default_value_t = DEFAULT_PORT, env = "NOVA_PORT")]
    port: u16,
    /// Print raw server responses without terminal formatting.
    #[arg(long)]
    raw: bool,
    /// Disable ANSI colors.
    #[arg(long)]
    no_color: bool,
    /// Disable interactive command history.
    #[arg(long)]
    no_history: bool,
    /// Command history file path.
    #[arg(long, env = "NOVA_CLI_HISTORY")]
    history_file: Option<PathBuf>,
    /// Command and arguments, for example: WRITE cpu 1000 42.5
    ///
    /// When omitted, nova-cli starts an interactive session against the server.
    #[arg(trailing_var_arg = true)]
    command: Vec<String>,
}

impl Arguments {
    fn server_address(&self) -> String {
        self.address
            .clone()
            .unwrap_or_else(|| format!("{}:{}", self.host, self.port))
    }
}

#[derive(Clone, Copy)]
struct OutputMode {
    color: bool,
    raw: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LocalCommand {
    Help,
    Clear,
    Quit,
}

struct ClientConnection {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
}

impl ClientConnection {
    async fn connect(address: &str) -> anyhow::Result<Self> {
        let stream = TcpStream::connect(address).await.with_context(|| {
            format!(
                "could not connect to Nova at {address}\n\n\
                 Start the server in another terminal:\n  \
                 cargo run -p nova-server\n\n\
                 Or point the CLI at a running server:\n  \
                 nova-cli --address <host:port>"
            )
        })?;
        let (reader, writer) = stream.into_split();
        Ok(Self {
            reader: BufReader::new(reader),
            writer,
        })
    }

    async fn send(&mut self, command: &str) -> anyhow::Result<String> {
        self.writer.write_all(command.as_bytes()).await?;
        self.writer.write_all(b"\n").await?;

        let mut response = String::new();
        let bytes = self.reader.read_line(&mut response).await?;
        anyhow::ensure!(bytes > 0, "connection closed by server");
        Ok(response.trim_end().to_owned())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let arguments = Arguments::parse();
    let _ = arguments.help;
    let address = arguments.server_address();
    let output = OutputMode {
        color: io::stdout().is_terminal() && !arguments.no_color,
        raw: arguments.raw,
    };

    if let Some(local) = parse_one_shot_local_command(&arguments.command) {
        run_local_command(local, output)?;
        return Ok(());
    }

    let mut connection = ClientConnection::connect(&address).await?;
    if arguments.command.is_empty() {
        if io::stdin().is_terminal() {
            run_repl(
                &mut connection,
                &address,
                output,
                history_path(arguments.no_history, arguments.history_file),
            )
            .await
        } else {
            run_stdin_commands(&mut connection, output).await
        }
    } else {
        let command = arguments.command.join(" ");
        let response = connection.send(&command).await?;
        print_response(&response, output);
        Ok(())
    }
}

async fn run_repl(
    connection: &mut ClientConnection,
    address: &str,
    output: OutputMode,
    history_path: Option<PathBuf>,
) -> anyhow::Result<()> {
    let mut editor = DefaultEditor::new().context("failed to initialize line editor")?;
    load_history(&mut editor, history_path.as_ref());

    println!("Nova CLI {VERSION} - connected to {address}");
    println!("Type HELP for commands, or QUIT to exit.");

    loop {
        match editor.readline("nova> ") {
            Ok(input) => {
                let input = input.trim();
                if input.is_empty() {
                    continue;
                }

                if !input.starts_with('.') {
                    let _ = editor.add_history_entry(input);
                }

                if let Some(local) = parse_local_command(input) {
                    if local == LocalCommand::Quit {
                        save_history(&mut editor, history_path.as_ref());
                        return Ok(());
                    }
                    run_local_command(local, output)?;
                    continue;
                }

                let response = connection.send(input).await?;
                print_response(&response, output);
            }
            Err(ReadlineError::Interrupted) => {
                println!("Use QUIT to exit.");
            }
            Err(ReadlineError::Eof) => {
                println!();
                save_history(&mut editor, history_path.as_ref());
                return Ok(());
            }
            Err(error) => return Err(error).context("failed to read interactive input"),
        }
    }
}

async fn run_stdin_commands(
    connection: &mut ClientConnection,
    output: OutputMode,
) -> anyhow::Result<()> {
    let mut input = String::new();
    while io::stdin().read_line(&mut input)? > 0 {
        let command = input.trim();
        if !command.is_empty() {
            if let Some(local) = parse_local_command(command) {
                if local == LocalCommand::Quit {
                    return Ok(());
                }
                run_local_command(local, output)?;
            } else {
                let response = connection.send(command).await?;
                print_response(&response, output);
            }
        }
        input.clear();
    }
    Ok(())
}

fn parse_one_shot_local_command(command: &[String]) -> Option<LocalCommand> {
    if command.len() == 1 {
        parse_local_command(&command[0])
    } else {
        None
    }
}

fn parse_local_command(input: &str) -> Option<LocalCommand> {
    match input.trim().to_ascii_lowercase().as_str() {
        "help" | "?" | ".help" => Some(LocalCommand::Help),
        "clear" | ".clear" => Some(LocalCommand::Clear),
        "quit" | "exit" | "q" | ".quit" | ".exit" => Some(LocalCommand::Quit),
        _ => None,
    }
}

fn run_local_command(command: LocalCommand, output: OutputMode) -> anyhow::Result<()> {
    match command {
        LocalCommand::Help => {
            println!("{}", command_help());
        }
        LocalCommand::Clear => {
            if output.raw {
                return Ok(());
            }
            print!("\x1b[2J\x1b[H");
            io::stdout().flush()?;
        }
        LocalCommand::Quit => {}
    }
    Ok(())
}

fn command_help() -> &'static str {
    "Nova CLI commands

Server commands:
  PING
  WRITE <metric> <timestamp-ms> <value>
  BATCH <metric> <timestamp-ms> <value> [<metric> <timestamp-ms> <value> ...]
  RANGE <metric> <start-ms> <end-ms>
  INFO

Local commands:
  HELP, ?, .help       Show this help
  CLEAR, .clear        Clear the terminal
  QUIT, EXIT, Q        Exit interactive mode

Connection:
  nova-cli --address 127.0.0.1:7422
  nova-cli -h 127.0.0.1 -p 7422
  NOVA_ADDRESS, NOVA_HOST, and NOVA_PORT are supported"
}

fn history_path(no_history: bool, explicit_path: Option<PathBuf>) -> Option<PathBuf> {
    if no_history {
        return None;
    }
    explicit_path.or_else(default_history_path)
}

fn default_history_path() -> Option<PathBuf> {
    env::var_os("HOME").map(|home| PathBuf::from(home).join(".nova-cli-history"))
}

fn load_history(editor: &mut DefaultEditor, path: Option<&PathBuf>) {
    let Some(path) = path else {
        return;
    };
    if path.exists() {
        let _ = editor.load_history(path);
    }
}

fn save_history(editor: &mut DefaultEditor, path: Option<&PathBuf>) {
    let Some(path) = path else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = editor.save_history(path);
}

fn print_response(response: &str, output: OutputMode) {
    println!("{}", format_response(response, output));
}

fn colorize(color: bool, code: &str, text: &str) -> String {
    if color {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_owned()
    }
}

/// Formats a raw protocol response for terminal display.
fn format_response(response: &str, output: OutputMode) -> String {
    let response = response.trim_end();
    if output.raw {
        return response.to_owned();
    }

    if let Some(reason) = response.strip_prefix("ERR ") {
        return colorize(output.color, "31", &format!("ERR {reason}"));
    }
    if response == "OK" || response == "PONG" {
        return colorize(output.color, "32", response);
    }
    if let Some(rest) = response.strip_prefix("POINTS ") {
        return format_points(rest, output.color);
    }
    if let Some(rest) = response.strip_prefix("INFO ") {
        return format_info(rest, output.color);
    }
    response.to_owned()
}

fn format_points(rest: &str, color: bool) -> String {
    let Some((count, body)) = rest.split_once(' ') else {
        if rest == "0" {
            return colorize(color, "90", "(0 points)");
        }
        return format!("POINTS {rest}");
    };

    let rows: Vec<(&str, &str)> = body
        .split(';')
        .filter(|pair| !pair.is_empty())
        .filter_map(|pair| pair.split_once(' '))
        .collect();

    if rows.is_empty() {
        return colorize(color, "90", "(0 points)");
    }

    let timestamp_width = rows
        .iter()
        .map(|(timestamp, _)| timestamp.len())
        .max()
        .unwrap_or("timestamp".len())
        .max("timestamp".len())
        + 2;

    let mut output = format!("{:<timestamp_width$}{}\n", "timestamp", "value");
    for (timestamp, value) in &rows {
        let _ = writeln!(output, "{timestamp:<timestamp_width$}{value}");
    }
    output.push_str(&colorize(color, "90", &format!("({count} points)")));
    output
}

fn format_info(rest: &str, color: bool) -> String {
    let fields: Vec<(&str, &str)> = rest
        .split_ascii_whitespace()
        .filter_map(|field| field.split_once('='))
        .collect();

    if fields.is_empty() {
        return colorize(color, "90", "(empty)");
    }

    let key_width = fields.iter().map(|(key, _)| key.len()).max().unwrap_or(0) + 2;
    let mut output = String::new();
    for (index, (key, value)) in fields.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        let _ = write!(output, "{key:<key_width$}{value}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{Arguments, LocalCommand, OutputMode, format_response, parse_local_command};
    use clap::Parser as _;

    fn plain_output() -> OutputMode {
        OutputMode {
            color: false,
            raw: false,
        }
    }

    #[test]
    fn builds_address_from_host_and_port() {
        let arguments = Arguments {
            help: None,
            address: None,
            host: "localhost".to_owned(),
            port: 9999,
            raw: false,
            no_color: false,
            no_history: false,
            history_file: None,
            command: Vec::new(),
        };

        assert_eq!(arguments.server_address(), "localhost:9999");
    }

    #[test]
    fn address_overrides_host_and_port() {
        let arguments = Arguments {
            help: None,
            address: Some("10.0.0.1:7000".to_owned()),
            host: "localhost".to_owned(),
            port: 9999,
            raw: false,
            no_color: false,
            no_history: false,
            history_file: None,
            command: Vec::new(),
        };

        assert_eq!(arguments.server_address(), "10.0.0.1:7000");
    }

    #[test]
    fn parses_short_host_and_port_flags() {
        let arguments =
            Arguments::try_parse_from(["nova-cli", "-h", "localhost", "-p", "9999", "PING"])
                .expect("short host and port flags should parse");

        assert_eq!(arguments.server_address(), "localhost:9999");
        assert_eq!(arguments.command, ["PING"]);
    }

    #[test]
    fn parses_address_override_with_trailing_command() {
        let arguments = Arguments::try_parse_from([
            "nova-cli",
            "--address",
            "10.0.0.1:7000",
            "-h",
            "localhost",
            "-p",
            "9999",
            "INFO",
        ])
        .expect("address override and trailing command should parse");

        assert_eq!(arguments.server_address(), "10.0.0.1:7000");
        assert_eq!(arguments.command, ["INFO"]);
    }

    #[test]
    fn recognizes_local_commands() {
        assert_eq!(parse_local_command("HELP"), Some(LocalCommand::Help));
        assert_eq!(parse_local_command("?"), Some(LocalCommand::Help));
        assert_eq!(parse_local_command(".clear"), Some(LocalCommand::Clear));
        assert_eq!(parse_local_command("q"), Some(LocalCommand::Quit));
        assert_eq!(parse_local_command("PING"), None);
    }

    #[test]
    fn formats_points_with_dynamic_columns() {
        assert_eq!(
            format_response("POINTS 2 1 2.5;100000 3.5", plain_output()),
            "timestamp  value\n1          2.5\n100000     3.5\n(2 points)"
        );
    }

    #[test]
    fn formats_zero_points() {
        assert_eq!(format_response("POINTS 0 ", plain_output()), "(0 points)");
    }

    #[test]
    fn formats_info_as_key_value_rows() {
        assert_eq!(
            format_response(
                "INFO version=0.1.0 git=unknown rustc=1.90.0 profile=debug metrics=1 points=2",
                plain_output()
            ),
            "version  0.1.0\ngit      unknown\nrustc    1.90.0\nprofile  debug\nmetrics  1\npoints   2"
        );
    }

    #[test]
    fn raw_output_skips_formatting() {
        assert_eq!(
            format_response(
                "POINTS 1 100 42",
                OutputMode {
                    color: false,
                    raw: true,
                },
            ),
            "POINTS 1 100 42"
        );
    }
}
