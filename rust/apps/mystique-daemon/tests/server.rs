//! End-to-end over the real transport: a client connects to the daemon's socket, sends
//! requests, and reads responses. The device is still fake; nothing touches hardware.

mod support;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use mystique_daemon::endpoint::Endpoint;
use mystique_daemon::server;
use mystique_daemon::worker::Worker;
use mystique_ipc::{
    cmd, decode, CommandOutcome, DetectedLabels, DeviceState, Request, Response, ServerMessage,
    ShutdownAck,
};
use mystique_sensors::SensorSample;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::{broadcast, watch};

use support::{build_core, fast_config, ProviderState};

static COUNTER: AtomicU64 = AtomicU64::new(0);

#[cfg(windows)]
fn test_endpoint() -> Endpoint {
    let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
    let name = format!("mystique-server-test-{}-{unique}", std::process::id());
    Endpoint::from_option(Some(&name)).unwrap()
}

#[cfg(unix)]
fn test_endpoint() -> Endpoint {
    let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = format!(
        "/tmp/mystique-server-test-{}-{unique}.sock",
        std::process::id()
    );
    Endpoint::from_option(Some(&path)).unwrap()
}

#[cfg(windows)]
async fn connect(endpoint: &Endpoint) -> impl AsyncRead + AsyncWrite + Send + 'static {
    use tokio::net::windows::named_pipe::ClientOptions;

    for _ in 0..300 {
        match ClientOptions::new().open(endpoint.pipe_name()) {
            Ok(client) => return client,
            Err(_) => tokio::time::sleep(Duration::from_millis(10)).await,
        }
    }
    panic!("the named pipe never accepted a connection");
}

#[cfg(unix)]
async fn connect(endpoint: &Endpoint) -> impl AsyncRead + AsyncWrite + Send + 'static {
    for _ in 0..300 {
        match tokio::net::UnixStream::connect(endpoint.path()).await {
            Ok(stream) => return stream,
            Err(_) => tokio::time::sleep(Duration::from_millis(10)).await,
        }
    }
    panic!("the Unix socket never accepted a connection");
}

async fn send<W: AsyncWrite + Unpin>(writer: &mut W, request: &Request) {
    let line = mystique_ipc::encode_line(request).unwrap();
    writer.write_all(line.as_bytes()).await.unwrap();
    writer.flush().await.unwrap();
}

async fn read_response<R: AsyncBufRead + Unpin>(reader: &mut R, id: u64) -> Response {
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).await.unwrap();
        assert!(read > 0, "the server closed the connection");
        match decode::<ServerMessage>(line.as_bytes()).unwrap() {
            ServerMessage::Response(response) if response.id == id => return response,
            ServerMessage::Response(_) | ServerMessage::Event(_) => continue,
        }
    }
}

#[tokio::test]
async fn a_client_round_trips_over_the_real_socket() {
    let state = std::sync::Arc::new(ProviderState::new());
    let now = Instant::now();
    let core = build_core(state.clone(), SensorSample::default(), fast_config(), now);
    let (events, _) = broadcast::channel(32);
    let worker = Worker::spawn(core, events.clone()).unwrap();

    let endpoint = test_endpoint();
    endpoint.prepare().unwrap();
    let (shutdown_tx, _shutdown_rx) = watch::channel(false);
    let mut server_task = tokio::spawn(server::serve(
        endpoint.clone(),
        worker.sender(),
        events,
        shutdown_tx.clone(),
    ));

    let stream = connect(&endpoint).await;
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = tokio::io::BufReader::new(reader);

    // 1. Open explicitly, so the test does not depend on the worker's first tick.
    send(&mut writer, &Request::new(1, cmd::OPEN_DEVICE)).await;
    let response = read_response(&mut reader, 1).await;
    assert!(response.ok, "open failed: {:?}", response.error);

    // 2. A settings command changes the session and reports the same DTO.
    let request = Request::with_args(
        2,
        cmd::SET_BRIGHTNESS,
        &mystique_ipc::args::Brightness { brightness: 33 },
    )
    .unwrap();
    send(&mut writer, &request).await;
    let response = read_response(&mut reader, 2).await;
    let outcome: CommandOutcome = response.into_data().unwrap();
    assert_eq!(outcome.settings.brightness, 33);
    assert_eq!(state.last_brightness(), Some(33));

    // 3. The state view agrees, and the daemon reports its telemetry stream.
    send(&mut writer, &Request::new(3, cmd::DEVICE_STATE)).await;
    let response = read_response(&mut reader, 3).await;
    let device: DeviceState = response.into_data().unwrap();
    assert!(device.device_open);
    assert_eq!(device.settings.brightness, 33);
    assert!(device.telemetry.expect("telemetry status").running);

    // 4. An unknown command is answered, not fatal.
    send(&mut writer, &Request::new(4, "set_everything")).await;
    let response = read_response(&mut reader, 4).await;
    assert!(!response.ok);
    assert_eq!(response.error.unwrap().code, "unknown_command");

    // 5. `detected_labels` is answered by the connection handler, not the device worker:
    // the session has no such dispatch (it would be `unknown_command`), so success here
    // proves the host probe never queues behind a frame.
    send(&mut writer, &Request::new(5, cmd::DETECTED_LABELS)).await;
    let response = read_response(&mut reader, 5).await;
    assert!(response.ok, "{:?}", response.error);
    let labels: DetectedLabels = response.into_data().unwrap();
    assert!(labels.cpu.len() <= 1, "{:?}", labels.cpu);

    // 6. Shut down cleanly: the socket task stops, the worker blanks and exits.
    drop(writer);
    drop(reader);
    shutdown_tx.send(true).unwrap();
    let stopped = tokio::time::timeout(Duration::from_secs(2), &mut server_task).await;
    assert!(stopped.is_ok(), "the server did not stop");

    let clean = tokio::time::timeout(Duration::from_secs(2), worker.shutdown()).await;
    assert!(
        clean.unwrap_or(false),
        "the worker did not acknowledge shutdown"
    );
    assert_eq!(
        state.last_brightness(),
        Some(0),
        "shutdown blanks the panel"
    );

    endpoint.cleanup();
}

/// The daemon's own stop command: the answer goes out, then the server ends on its own,
/// then the worker releases the panel. No signal and no UI are involved.
#[tokio::test]
async fn a_shutdown_request_is_answered_before_the_daemon_stops() {
    let state = std::sync::Arc::new(ProviderState::new());
    let now = Instant::now();
    let core = build_core(state.clone(), SensorSample::default(), fast_config(), now);
    let (events, _) = broadcast::channel(32);
    let worker = Worker::spawn(core, events.clone()).unwrap();

    let endpoint = test_endpoint();
    endpoint.prepare().unwrap();
    let (shutdown_tx, _shutdown_rx) = watch::channel(false);
    let mut server_task = tokio::spawn(server::serve(
        endpoint.clone(),
        worker.sender(),
        events,
        shutdown_tx,
    ));

    let stream = connect(&endpoint).await;
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = tokio::io::BufReader::new(reader);

    send(&mut writer, &Request::new(1, cmd::OPEN_DEVICE)).await;
    let opened = read_response(&mut reader, 1).await;
    assert!(opened.ok, "open failed: {:?}", opened.error);

    send(&mut writer, &Request::new(2, cmd::SHUTDOWN)).await;
    let response = read_response(&mut reader, 2).await;
    let ack: ShutdownAck = response.into_data().expect("the shutdown is acknowledged");
    assert!(ack.stopping);
    assert!(ack.message.contains("panel"), "{}", ack.message);

    // No `shutdown_tx.send` in this test: the command itself must end the accept loop.
    let stopped = tokio::time::timeout(Duration::from_secs(2), &mut server_task).await;
    assert!(stopped.is_ok(), "the server did not stop after `shutdown`");
    assert!(stopped.unwrap().expect("the server task joined").is_ok());

    let clean = tokio::time::timeout(Duration::from_secs(2), worker.shutdown()).await;
    assert!(
        clean.unwrap_or(false),
        "the worker did not acknowledge shutdown"
    );
    assert_eq!(
        state.last_brightness(),
        Some(0),
        "the daemon does not leave the device lit or open"
    );
    assert!(
        !state.commands.lock().expect("fake command log").is_empty(),
        "the clean stop reached the panel"
    );

    endpoint.cleanup();
    #[cfg(unix)]
    assert!(
        !endpoint.path().exists(),
        "the clean stop removes the Unix socket"
    );
}
