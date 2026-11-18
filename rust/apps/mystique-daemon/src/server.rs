//! The accept loop and the platform endpoint.
//!
//! Windows creates one named-pipe instance per client; Unix accepts one `UnixStream`
//! per client. In both cases each connection is handled by [`crate::client::handle`],
//! which owns no device state. The accept task is separate from the device worker, so a
//! client that connects, stalls, or floods never delays a telemetry frame. A `shutdown`
//! request breaks this loop through the same watch channel the signal path uses.

use std::io;

use tokio::sync::{broadcast, watch};

use mystique_ipc::ServerMessage;

use crate::endpoint::Endpoint;
use crate::worker::WorkerSender;

#[cfg(windows)]
pub async fn serve(
    endpoint: Endpoint,
    worker: WorkerSender,
    events: broadcast::Sender<ServerMessage>,
    shutdown: watch::Sender<bool>,
) -> io::Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;

    let pipe = endpoint.pipe_name().to_owned();
    let mut server = ServerOptions::new().create(&pipe)?;
    let mut shutdown_rx = shutdown.subscribe();
    let mut clients = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            connected = server.connect() => {
                connected?;
                let connected = server;
                // Create the next instance before handling this one, so a client that
                // connects while the previous one is still loading is not refused.
                server = ServerOptions::new().create(&pipe)?;
                let worker = worker.clone();
                let events = events.clone();
                let shutdown = shutdown.clone();
                clients.spawn(async move {
                    crate::client::handle(connected, worker, events, shutdown).await;
                });
            }
        }
    }
    clients.abort_all();
    while clients.join_next().await.is_some() {}
    Ok(())
}

#[cfg(unix)]
pub async fn serve(
    endpoint: Endpoint,
    worker: WorkerSender,
    events: broadcast::Sender<ServerMessage>,
    shutdown: watch::Sender<bool>,
) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let listener = tokio::net::UnixListener::bind(endpoint.path())?;
    std::fs::set_permissions(endpoint.path(), std::fs::Permissions::from_mode(0o600))?;
    let mut shutdown_rx = shutdown.subscribe();
    let mut clients = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            accepted = listener.accept() => {
                let (stream, _address) = accepted?;
                let worker = worker.clone();
                let events = events.clone();
                let shutdown = shutdown.clone();
                clients.spawn(async move {
                    crate::client::handle(stream, worker, events, shutdown).await;
                });
            }
        }
    }
    clients.abort_all();
    while clients.join_next().await.is_some() {}
    Ok(())
}
