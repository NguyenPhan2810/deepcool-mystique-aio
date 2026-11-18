//! The connection loop: connect, serve lines, reconnect with backoff.
//!
//! One request at a time is written from the caller side, but arrival order is never
//! assumed: every response is matched by `id` against a pending table, so an event
//! between two responses, or two responses in the opposite order, cannot mis-deliver a
//! payload. Events are forwarded to the notice channel and never kept here.
//!
//! A write that makes no progress for the request deadline is treated as a wedged pipe:
//! the connection is dropped and reopened. A response that never arrives is the caller's
//! timeout, and the caller reports it without pretending the link is gone.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use mystique_ipc::{CommandError, Event, LineDecoder, Request, Response, ServerMessage};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};

use super::transport::Transport;
use super::{Call, LinkState, Notice};
use crate::error::{daemon_timeout, daemon_unavailable};

/// First pause before a reconnect attempt. Short: the common case is a daemon that has
/// just been started.
const BACKOFF_START: Duration = Duration::from_millis(250);
/// Ceiling for the reconnect pause.
const BACKOFF_MAX: Duration = Duration::from_secs(5);

type Entry = (Instant, oneshot::Sender<Result<Response, CommandError>>);
type PendingMap = HashMap<u64, Entry>;
type Pending = Arc<Mutex<PendingMap>>;

enum Outcome {
    Reconnect,
    Stop,
}

enum Idle {
    Stop,
    RetryNow,
    Waited,
}

pub(super) async fn run(
    transport: Arc<dyn Transport>,
    mut calls: mpsc::UnboundedReceiver<Call>,
    notices: mpsc::UnboundedSender<Notice>,
    link: Arc<Mutex<LinkState>>,
    timeout: Duration,
) {
    let endpoint = transport.endpoint();
    let mut backoff = BACKOFF_START;

    loop {
        publish(&link, &notices, LinkState::connecting(&endpoint));
        match transport.connect(timeout).await {
            Ok(stream) => {
                backoff = BACKOFF_START;
                publish(&link, &notices, LinkState::connected(&endpoint));
                if let Outcome::Stop = serve(stream, &mut calls, &notices, timeout).await {
                    return;
                }
                publish(
                    &link,
                    &notices,
                    LinkState::disconnected(&endpoint, "the daemon closed the connection"),
                );
            }
            Err(error) => {
                publish(
                    &link,
                    &notices,
                    LinkState::disconnected(&endpoint, error.to_string()),
                );
            }
        }

        match idle(&mut calls, backoff).await {
            Idle::Stop => return,
            Idle::RetryNow => backoff = BACKOFF_START,
            Idle::Waited => backoff = grow(backoff),
        }
    }
}

fn grow(backoff: Duration) -> Duration {
    (backoff * 2).min(BACKOFF_MAX)
}

fn publish(link: &Mutex<LinkState>, notices: &mpsc::UnboundedSender<Notice>, state: LinkState) {
    *link.lock().unwrap_or_else(PoisonError::into_inner) = state.clone();
    let _ = notices.send(Notice::Link(state));
}

/// Waits between attempts while still answering requests and a manual reconnect.
async fn idle(calls: &mut mpsc::UnboundedReceiver<Call>, wait: Duration) -> Idle {
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        tokio::select! {
            call = calls.recv() => match call {
                None => return Idle::Stop,
                Some(Call::Shutdown) => return Idle::Stop,
                Some(Call::Reconnect) => return Idle::RetryNow,
                Some(Call::Request { reply, .. }) => {
                    let _ = reply.send(Err(daemon_unavailable(
                        "the daemon is not reachable; the client is retrying",
                    )));
                }
            },
            _ = tokio::time::sleep(remaining) => return Idle::Waited,
        }
    }
}

async fn serve(
    stream: Box<dyn super::transport::Stream>,
    calls: &mut mpsc::UnboundedReceiver<Call>,
    notices: &mpsc::UnboundedSender<Notice>,
    timeout: Duration,
) -> Outcome {
    let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
    let (reader, mut writer) = tokio::io::split(stream);
    let (closed_tx, mut closed_rx) = oneshot::channel::<()>();
    let reader_task = tokio::spawn(read_loop(
        reader,
        Arc::clone(&pending),
        notices.clone(),
        closed_tx,
    ));

    let outcome = loop {
        tokio::select! {
            biased;
            call = calls.recv() => match call {
                None | Some(Call::Shutdown) => break Outcome::Stop,
                // Already connected: there is nothing to repair, and dropping a healthy
                // link would only interrupt the stream. Reconnect matters while idle.
                Some(Call::Reconnect) => {}
                Some(Call::Request { id, cmd, args, deadline, reply }) => {
                    // Only already-abandoned callers are pruned: their own timeout has
                    // fired, so dropping the sender cannot strand anyone.
                    prune(&pending, Instant::now());
                    pending
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(id, (deadline, reply));

                    let request = Request { id, cmd, args };
                    let line = match mystique_ipc::encode(&request) {
                        Ok(line) => line,
                        Err(error) => {
                            if let Some((_, reply)) = take(&pending, id) {
                                let _ = reply.send(Err(CommandError::internal(error.to_string())));
                            }
                            continue;
                        }
                    };
                    match tokio::time::timeout(timeout, writer.write_all(&line)).await {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => {
                            fail(&pending, daemon_unavailable(format!(
                                "the connection failed while writing: {error}"
                            )));
                            break Outcome::Reconnect;
                        }
                        Err(_) => {
                            fail(&pending, daemon_timeout(timeout));
                            break Outcome::Reconnect;
                        }
                    }
                }
            },
            _ = &mut closed_rx => {
                fail(&pending, daemon_unavailable(
                    "the daemon closed the connection before answering",
                ));
                break Outcome::Reconnect;
            }
        }
    };

    reader_task.abort();
    outcome
}

async fn read_loop<R>(
    mut reader: R,
    pending: Pending,
    notices: mpsc::UnboundedSender<Notice>,
    closed: oneshot::Sender<()>,
) where
    R: AsyncRead + Unpin,
{
    let mut decoder = LineDecoder::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = match reader.read(&mut buffer).await {
            Ok(0) => break,
            Ok(count) => count,
            Err(_) => break,
        };
        let messages = match decoder.feed::<ServerMessage>(&buffer[..read]) {
            Ok(messages) => messages,
            // A frame this client cannot parse means the two sides disagree about the
            // protocol. Dropping the connection is the only safe answer: the stream
            // cannot be resynchronised mid-line.
            Err(_) => break,
        };
        for message in messages {
            match message {
                ServerMessage::Response(response) => {
                    if let Some((_, reply)) = take(&pending, response.id) {
                        let _ = reply.send(Ok(response));
                    }
                }
                ServerMessage::Event(Event::Telemetry { frame }) => {
                    if notices.send(Notice::Telemetry(frame)).is_err() {
                        return;
                    }
                }
                ServerMessage::Event(Event::State { state }) => {
                    if notices.send(Notice::State(state)).is_err() {
                        return;
                    }
                }
            }
        }
    }
    let _ = closed.send(());
}

fn pending_guard(pending: &Pending) -> MutexGuard<'_, PendingMap> {
    pending.lock().unwrap_or_else(PoisonError::into_inner)
}

fn take(pending: &Pending, id: u64) -> Option<Entry> {
    pending_guard(pending).remove(&id)
}

fn prune(pending: &Pending, now: Instant) {
    pending_guard(pending).retain(|_, (deadline, _)| *deadline > now);
}

/// Drops every waiting caller with the same error. Used when the link goes away.
fn fail(pending: &Pending, error: CommandError) {
    let waiting = std::mem::take(&mut *pending_guard(pending));
    for (_, reply) in waiting.into_values() {
        let _ = reply.send(Err(error.clone()));
    }
}
