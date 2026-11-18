//! One connected client: a reader that turns lines into requests, and a writer that
//! interleaves that client's responses with the broadcast events.
//!
//! A slow client cannot block the telemetry loop. Events go through a bounded
//! `broadcast` channel; a receiver that falls behind gets `Lagged`, and this module
//! disconnects it instead of serving stale frames. A client that stops reading its
//! socket is stopped by a write timeout. The request path is separately backpressured
//! by its own bounded response queue.
//!
//! `shutdown` is the one command answered here rather than by the device worker: a stop
//! request must not queue behind a full request queue or a wedged transfer. Its reply is
//! flushed first, then the process-wide stop signal is set.

use std::time::Duration;

use mystique_ipc::{cmd, CommandError, LineDecoder, Request, Response, ServerMessage, ShutdownAck};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use crate::worker::WorkerSender;

/// Responses waiting for one client's socket. Small on purpose: a client that cannot
/// keep up with its own answers is not a client worth buffering for.
const RESPONSE_QUEUE: usize = 32;

/// How long one write may make no progress before the client is dropped.
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// One line queued for a client. `written` is set only for the `shutdown` answer: the
/// daemon must know the line left the socket before it starts tearing the process down.
struct Outbound {
    message: ServerMessage,
    written: Option<oneshot::Sender<()>>,
}

impl Outbound {
    fn new(message: ServerMessage) -> Self {
        Self {
            message,
            written: None,
        }
    }
}

/// Handles one connection until the client disconnects, sends a bad frame, falls too far
/// behind on events, or asks the daemon to stop. `shutdown` is the process-wide stop
/// signal: it is set only after the `shutdown` answer has been written.
pub async fn handle<S>(
    stream: S,
    worker: WorkerSender,
    events: broadcast::Sender<ServerMessage>,
    shutdown: watch::Sender<bool>,
) where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (mut reader, writer) = tokio::io::split(stream);
    let (responses_tx, responses_rx) = mpsc::channel::<Outbound>(RESPONSE_QUEUE);
    let subscription = events.subscribe();
    let writer_task = tokio::spawn(write_loop(writer, responses_rx, subscription));

    let mut decoder = LineDecoder::new();
    let mut buffer = [0u8; 8192];
    'read: loop {
        let read = match reader.read(&mut buffer).await {
            Ok(0) => break,
            Ok(count) => count,
            Err(_) => break,
        };
        let requests = match decoder.feed::<Request>(&buffer[..read]) {
            Ok(requests) => requests,
            Err(error) => {
                // There is no id to answer with: the frame did not parse. Close.
                eprintln!("mystique-daemon: closing a client that sent a bad frame: {error}");
                break;
            }
        };
        for request in requests {
            let id = request.id;
            if request.cmd == cmd::SHUTDOWN {
                // Answered here, not by the device worker: a stop must not wait behind a
                // full queue or a wedged transfer. Nothing has been released yet, and
                // the acknowledgement says only that the request was accepted.
                let ack = ShutdownAck {
                    stopping: true,
                    message: "the daemon is stopping: telemetry stops, the panel is \
                              released, and the control endpoint is removed"
                        .to_owned(),
                };
                let response = Response::success(id, &ack).unwrap_or_else(|error| {
                    Response::failure(id, CommandError::internal(error.to_string()))
                });
                let (written, acknowledgement) = oneshot::channel();
                let outbound = Outbound {
                    message: ServerMessage::Response(response),
                    written: Some(written),
                };
                if responses_tx.send(outbound).await.is_err() {
                    break;
                }
                // Reply first. Only once the writer says the line is on the wire does the
                // process-wide stop begin, so the client that asked is never left
                // guessing whether its request arrived.
                let _ = tokio::time::timeout(WRITE_TIMEOUT, acknowledgement).await;
                let _ = shutdown.send(true);
                break 'read;
            }

            let response = match worker.request(request) {
                Ok(receiver) => match receiver.await {
                    Ok(response) => response,
                    Err(_) => Response::failure(
                        id,
                        CommandError::internal("the device worker stopped before answering"),
                    ),
                },
                Err(error) => Response::failure(id, error),
            };
            let outbound = Outbound::new(ServerMessage::Response(response));
            if responses_tx.send(outbound).await.is_err() {
                break;
            }
        }
    }

    drop(responses_tx);
    writer_task.abort();
}

async fn write_loop<S>(
    mut writer: S,
    mut responses: mpsc::Receiver<Outbound>,
    mut events: broadcast::Receiver<ServerMessage>,
) where
    S: AsyncWrite + Unpin,
{
    loop {
        let Some(outbound) = next_message(&mut events, &mut responses).await else {
            break;
        };
        let line = match outbound.message.encode() {
            Ok(line) => line,
            Err(error) => {
                eprintln!("mystique-daemon: dropping a client: {error}");
                break;
            }
        };
        match tokio::time::timeout(WRITE_TIMEOUT, writer.write_all(&line)).await {
            Ok(Ok(())) => {
                if let Some(written) = outbound.written {
                    let _ = written.send(());
                }
            }
            Ok(Err(_)) => break,
            Err(_) => {
                eprintln!("mystique-daemon: dropping a client that stopped reading");
                break;
            }
        }
    }
}

/// Picks the next message for a client: its own responses first come from `responses`,
/// and broadcast events from the shared subscription. `None` means disconnect.
async fn next_message(
    events: &mut broadcast::Receiver<ServerMessage>,
    responses: &mut mpsc::Receiver<Outbound>,
) -> Option<Outbound> {
    tokio::select! {
        event = events.recv() => match event {
            Ok(message) => Some(Outbound::new(message)),
            Err(broadcast::error::RecvError::Lagged(dropped)) => {
                eprintln!(
                    "mystique-daemon: dropping a slow client that fell {dropped} events behind"
                );
                None
            }
            Err(broadcast::error::RecvError::Closed) => None,
        },
        response = responses.recv() => response,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mystique_ipc::{DeviceState, Event, SettingsState};
    use tokio::io::AsyncBufReadExt;

    fn state() -> DeviceState {
        DeviceState {
            device_open: true,
            device_present: true,
            service_running: false,
            service_supported: true,
            service_output: String::new(),
            last_error: None,
            rotation: Some(0),
            settings: SettingsState::default(),
            telemetry: None,
        }
    }

    #[tokio::test]
    async fn a_client_that_falls_behind_is_disconnected() {
        let (events, _keep_alive) = broadcast::channel(2);
        let mut subscription = events.subscribe();
        let (_responses_tx, mut responses) = mpsc::channel(2);

        // The receiver never drains; the channel holds two and the rest are dropped.
        for _ in 0..10 {
            let _ = events.send(ServerMessage::Event(Event::state(state())));
        }

        let message = next_message(&mut subscription, &mut responses).await;
        assert!(
            message.is_none(),
            "a lagging client must be dropped, not served stale events"
        );
    }

    #[tokio::test]
    async fn a_response_is_delivered_even_when_no_event_fires() {
        let (events, _keep_alive) = broadcast::channel(2);
        let mut subscription = events.subscribe();
        let (responses_tx, mut responses) = mpsc::channel(2);
        let response = Response::success(9, &"ok").unwrap();
        responses_tx
            .send(Outbound::new(ServerMessage::Response(response)))
            .await
            .unwrap();

        let message = next_message(&mut subscription, &mut responses).await;
        assert!(matches!(
            message.map(|outbound| outbound.message),
            Some(ServerMessage::Response(_))
        ));
    }

    #[tokio::test]
    async fn a_reply_carrying_an_acknowledgement_confirms_only_after_the_write() {
        let (ours, theirs) = tokio::io::duplex(1024);
        let (events, _keep_alive) = broadcast::channel(2);
        let subscription = events.subscribe();
        let (responses_tx, responses_rx) = mpsc::channel(2);
        let writer = tokio::spawn(write_loop(ours, responses_rx, subscription));

        let (written, acknowledgement) = oneshot::channel();
        let response = Response::success(7, &"bye").unwrap();
        responses_tx
            .send(Outbound {
                message: ServerMessage::Response(response),
                written: Some(written),
            })
            .await
            .unwrap();

        let mut reader = tokio::io::BufReader::new(theirs);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert!(line.contains("\"id\":7"), "{line}");
        assert!(
            acknowledgement.await.is_ok(),
            "the daemon must be told the reply reached the socket"
        );

        drop(responses_tx);
        writer.abort();
    }
}
