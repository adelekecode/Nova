use anyhow::Context;
use clap::Parser;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

#[derive(Debug, Parser)]
#[command(name = "nova-cli", version, about = "Send a command to Nova")]
struct Arguments {
    /// Nova server address.
    #[arg(long, default_value = "127.0.0.1:7422")]
    address: String,
    /// Command and arguments, for example: WRITE cpu 1000 42.5
    #[arg(required = true, trailing_var_arg = true)]
    command: Vec<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let arguments = Arguments::parse();
    let mut stream = TcpStream::connect(&arguments.address)
        .await
        .with_context(|| format!("could not connect to Nova at {}", arguments.address))?;
    let command = arguments.command.join(" ");
    stream.write_all(command.as_bytes()).await?;
    stream.write_all(b"\n").await?;

    let mut response = String::new();
    BufReader::new(stream).read_line(&mut response).await?;
    print!("{response}");
    Ok(())
}
