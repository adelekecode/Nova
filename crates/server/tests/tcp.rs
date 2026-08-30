use std::{net::SocketAddr, sync::Arc, time::Duration};

use nova_engine::Engine;
use nova_server::{ServerLimits, serve_with_limits_until_shutdown};
use tempfile::tempdir;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::{Mutex, oneshot},
};

async fn start_server() -> (SocketAddr, oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
    start_server_with_limits(ServerLimits::default()).await
}

async fn start_server_with_limits(
    limits: ServerLimits,
) -> (SocketAddr, oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
    let directory = tempdir().expect("temporary data directory");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind server");
    let address = listener.local_addr().expect("local address");
    let engine = Arc::new(Mutex::new(
        Engine::open(directory.path()).expect("open engine"),
    ));
    let (shutdown_sender, shutdown_receiver) = oneshot::channel();

    let server = tokio::spawn(async move {
        let result = serve_with_limits_until_shutdown(listener, engine, limits, async {
            let _ = shutdown_receiver.await;
            Ok(())
        })
        .await;
        assert!(result.is_ok(), "server failed: {result:?}");
        drop(directory);
    });

    (address, shutdown_sender, server)
}

async fn send(address: SocketAddr, command: &str) -> String {
    let responses = send_all(address, &[command]).await;
    responses.into_iter().next().expect("one response")
}

async fn send_all(address: SocketAddr, commands: &[&str]) -> Vec<String> {
    let stream = TcpStream::connect(address)
        .await
        .expect("connect to server");
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let mut responses = Vec::with_capacity(commands.len());

    for command in commands {
        writer
            .write_all(format!("{command}\n").as_bytes())
            .await
            .expect("write command");
        responses.push(
            lines
                .next_line()
                .await
                .expect("read response")
                .expect("server response"),
        );
    }

    responses
}

#[tokio::test]
async fn serves_real_tcp_commands() {
    let (address, shutdown, server) = start_server().await;

    let responses = send_all(
        address,
        &[
            "PING",
            "WRITE cpu.usage 100 1.5",
            "WRITE cpu.usage 200 2.5",
            "BATCH mem.used 100 4.5 mem.used 200 5.5",
            "RANGE cpu.usage 0 200",
            "RANGE mem.used 0 200",
        ],
    )
    .await;
    assert_eq!(
        responses,
        [
            "PONG",
            "OK",
            "OK",
            "OK",
            "POINTS 2 100 1.5;200 2.5",
            "POINTS 2 100 4.5;200 5.5"
        ]
    );

    let info = send(address, "INFO").await;
    assert!(info.starts_with("INFO version="), "{info}");
    assert!(info.contains(" metrics=2 "), "{info}");
    assert!(info.ends_with(" points=4"), "{info}");

    shutdown.send(()).expect("send shutdown");
    server.await.expect("server task");
}

#[tokio::test]
async fn reports_structured_errors_over_tcp() {
    let (address, shutdown, server) = start_server().await;

    assert_eq!(
        send(address, "RANGE cpu.usage 200 100").await,
        "ERR INVALID_RANGE range start must be less than or equal to range end"
    );
    assert_eq!(
        send(address, "WRITE bad/name 100 1.5").await,
        "ERR INVALID_METRIC invalid metric name"
    );
    assert_eq!(
        send(address, "NOPE").await,
        "ERR UNKNOWN_COMMAND unknown command"
    );

    shutdown.send(()).expect("send shutdown");
    server.await.expect("server task");
}

#[tokio::test]
async fn invalid_batch_does_not_partially_apply() {
    let (address, shutdown, server) = start_server().await;

    assert_eq!(
        send(address, "BATCH cpu.usage 100 1.5 bad/name 100 2.5").await,
        "ERR INVALID_METRIC invalid metric name"
    );
    assert_eq!(send(address, "RANGE cpu.usage 0 200").await, "POINTS 0 ");

    shutdown.send(()).expect("send shutdown");
    server.await.expect("server task");
}

#[tokio::test]
async fn shutdown_closes_idle_connections() {
    let (address, shutdown, server) = start_server().await;
    let mut stream = TcpStream::connect(address)
        .await
        .expect("connect to server");

    stream.write_all(b"PING\n").await.expect("write ping");
    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response).await.expect("read pong");
    assert_eq!(response.trim_end(), "PONG");

    shutdown.send(()).expect("send shutdown");

    let mut buffer = [0_u8; 1];
    assert_eq!(reader.read(&mut buffer).await.expect("read eof"), 0);
    server.await.expect("server task");
}

#[tokio::test]
async fn rejects_oversized_requests() {
    let limits = ServerLimits {
        max_request_bytes: 8,
        ..ServerLimits::default()
    };
    let (address, shutdown, server) = start_server_with_limits(limits).await;

    assert_eq!(
        send(address, "PING plus-extra").await,
        "ERR REQUEST_TOO_LARGE request exceeds maximum of 8 bytes"
    );

    shutdown.send(()).expect("send shutdown");
    server.await.expect("server task");
}

#[tokio::test]
async fn rejects_connections_over_the_limit() {
    let limits = ServerLimits {
        max_connections: 1,
        max_request_bytes: 1024,
        ..ServerLimits::default()
    };
    let (address, shutdown, server) = start_server_with_limits(limits).await;
    let first = TcpStream::connect(address)
        .await
        .expect("connect first client");
    let (reader, mut writer) = first.into_split();
    let mut lines = BufReader::new(reader).lines();

    writer.write_all(b"PING\n").await.expect("write ping");
    assert_eq!(
        lines
            .next_line()
            .await
            .expect("read first response")
            .expect("first response"),
        "PONG"
    );

    let second = TcpStream::connect(address)
        .await
        .expect("connect second client");
    let mut second_lines = BufReader::new(second).lines();
    assert_eq!(
        second_lines
            .next_line()
            .await
            .expect("read rejection")
            .expect("rejection response"),
        "ERR TOO_MANY_CONNECTIONS maximum connection limit reached"
    );

    shutdown.send(()).expect("send shutdown");
    server.await.expect("server task");
}

#[tokio::test]
async fn closes_idle_connections_after_timeout() {
    let limits = ServerLimits {
        client_idle_timeout: Duration::from_millis(50),
        ..ServerLimits::default()
    };
    let (address, shutdown, server) = start_server_with_limits(limits).await;
    let stream = TcpStream::connect(address)
        .await
        .expect("connect idle client");
    let mut lines = BufReader::new(stream).lines();

    assert_eq!(
        lines
            .next_line()
            .await
            .expect("read idle timeout")
            .expect("idle timeout response"),
        "ERR IDLE_TIMEOUT connection idle for more than 50 ms"
    );
    assert!(
        lines
            .next_line()
            .await
            .expect("read eof after idle timeout")
            .is_none()
    );

    shutdown.send(()).expect("send shutdown");
    server.await.expect("server task");
}
