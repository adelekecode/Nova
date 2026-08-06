use std::io::{self, IsTerminal, Write};

use anyhow::Context;
use clap::Parser;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Parser)]
#[command(name = "nova-cli", version, about = "Send commands to Nova")]
struct Arguments {
    /// Nova server address.
    #[arg(long, default_value = "127.0.0.1:7422")]
    address: String,
    /// Command and arguments, for example: WRITE cpu 1000 42.5
    ///
    /// When omitted, nova-cli starts an interactive session against the server.
    #[arg(trailing_var_arg = true)]
    command: Vec<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let arguments = Arguments::parse();
    let stream = TcpStream::connect(&arguments.address)
        .await
        .with_context(|| format!("could not connect to Nova at {}", arguments.address))?;
    let color = io::stdout().is_terminal();

    if arguments.command.is_empty() {
        run_repl(stream, &arguments.address, color).await
    } else {
        let command = arguments.command.join(" ");
        let response = send(stream, &command).await?;
        println!("{}", format_response(&response, color));
        Ok(())
    }
}

/// Runs an interactive session over a single, reused connection, similar to `redis-cli`.
async fn run_repl(stream: TcpStream, address: &str, color: bool) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    println!("Nova CLI {VERSION} - connected to {address}");
    println!("Type a command (PING, WRITE, RANGE, INFO) or 'quit' to exit.");

    loop {
        print!("nova> ");
        let _ = io::stdout().flush();

        let mut input = String::new();
        if io::stdin().read_line(&mut input)? == 0 {
            println!();
            return Ok(());
        }
        let input = input.trim();
        if input.is_empty() {
            continue;
        }
        if input.eq_ignore_ascii_case("quit") || input.eq_ignore_ascii_case("exit") {
            return Ok(());
        }

        writer.write_all(input.as_bytes()).await?;
        writer.write_all(b"\n").await?;

        match lines.next_line().await? {
            Some(response) => println!("{}", format_response(&response, color)),
            None => {
                println!("connection closed by server");
                return Ok(());
            }
        }
    }
}

/// Sends one command over a fresh connection and returns the single-line response.
async fn send(stream: TcpStream, command: &str) -> anyhow::Result<String> {
    let (reader, mut writer) = stream.into_split();
    writer.write_all(command.as_bytes()).await?;
    writer.write_all(b"\n").await?;

    let mut response = String::new();
    BufReader::new(reader).read_line(&mut response).await?;
    Ok(response.trim_end().to_owned())
}

fn colorize(color: bool, code: &str, text: &str) -> String {
    if color {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_owned()
    }
}

/// Formats a raw protocol response for terminal display: color-coded status, and a table for
/// `RANGE` results.
fn format_response(response: &str, color: bool) -> String {
    let response = response.trim_end();

    if let Some(reason) = response.strip_prefix("ERR ") {
        return colorize(color, "31", &format!("ERR {reason}"));
    }
    if response == "OK" || response == "PONG" {
        return colorize(color, "32", response);
    }
    if let Some(rest) = response.strip_prefix("POINTS ") {
        return format_points(rest, color);
    }
    if let Some(rest) = response.strip_prefix("INFO ") {
        return format_info(rest, color);
    }
    response.to_owned()
}

fn format_points(rest: &str, color: bool) -> String {
    let Some((count, body)) = rest.split_once(' ') else {
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

    let mut output = format!("{:<14}{}\n", "timestamp", "value");
    for (timestamp, value) in &rows {
        output.push_str(&format!("{timestamp:<14}{value}\n"));
    }
    output.push_str(&colorize(color, "90", &format!("({count} points)")));
    output
}

fn format_info(rest: &str, color: bool) -> String {
    let mut output = String::new();
    for (index, field) in rest.split_ascii_whitespace().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        if let Some((key, value)) = field.split_once('=') {
            output.push_str(&format!("{key:<10}{value}"));
        } else {
            output.push_str(field);
        }
    }
    if output.is_empty() {
        colorize(color, "90", "(empty)")
    } else {
        output
    }
}
