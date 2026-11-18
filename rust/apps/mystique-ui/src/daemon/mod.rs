//! The client end of the daemon's control socket.
//!
//! The UI holds no device handle, no session and no sampler. It sends requests and
//! renders what the daemon broadcasts; the process that owns the panel is elsewhere, and
//! closing this window does not end the stream.
//!
//! # Shape
//!
//! [`DaemonClient`] is a cheap, cloneable handle. A background thread owns a
//! current-thread runtime and one connection at a time; a reader task matches responses
//! by `id` and forwards [`Event`](mystique_ipc::Event)s as [`Notice`]s. Every command is
//! one request, and a request that cannot be answered fails with a code the UI can show.
//!
//! Nothing in this module knows about Tauri. `events` is the only place that emits to the
//! webview, which is what makes the whole client testable against an in-memory fake.

#[cfg(test)]
mod fake;
mod health;
mod process;
mod supervisor;
#[cfg(test)]
mod tests;
mod transport;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use mystique_ipc::{CommandError, DeviceState, FrameValues, Response};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

pub use health::{health, Health, Inputs as HealthInputs};
pub use process::{Launcher, SpawnRecord, STARTUP_WAIT};
pub use transport::{platform, Transport};

use crate::error::{daemon_timeout, daemon_unavailable};

/// How long one request may go unanswered. The daemon's own write timeout is 10 s, so a
/// client that waits longer can only be waiting on a wedged panel read.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Which control endpoint this client talks to, and how that link is doing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum LinkState {
    Connecting { endpoint: String },
    Connected { endpoint: String },
    Disconnected { endpoint: String, reason: String },
}

impl LinkState {
    pub fn connecting(endpoint: impl Into<String>) -> Self {
        Self::Connecting {
            endpoint: endpoint.into(),
        }
    }

    pub fn connected(endpoint: impl Into<String>) -> Self {
        Self::Connected {
            endpoint: endpoint.into(),
        }
    }

    pub fn disconnected(endpoint: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Disconnected {
            endpoint: endpoint.into(),
            reason: reason.into(),
        }
    }

    pub fn endpoint(&self) -> &str {
        match self {
            Self::Connecting { endpoint }
            | Self::Connected { endpoint }
            | Self::Disconnected { endpoint, .. } => endpoint,
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected { .. })
    }
}

/// Everything the client tells the rest of the app about. Connection changes and faults
/// are notices too, so the UI never has to poll to learn that the daemon went away.
pub enum Notice {
    Link(LinkState),
    Telemetry(FrameValues),
    State(Box<DeviceState>),
    Fault(CommandError),
}

/// One outbound message for the connection thread.
enum Call {
    Request {
        id: u64,
        cmd: String,
        args: Value,
        /// When the supervisor may forget this entry. Twice the caller's timeout, so a
        /// pruned entry always belongs to a caller that has already given up.
        deadline: std::time::Instant,
        reply: oneshot::Sender<Result<Response, CommandError>>,
    },
    Reconnect,
    Shutdown,
}

struct Inner {
    calls: mpsc::UnboundedSender<Call>,
    notices: mpsc::UnboundedSender<Notice>,
    next_id: AtomicU64,
    timeout: Duration,
    link: Arc<Mutex<LinkState>>,
}

/// A cloneable handle to the control connection. Dropping every clone leaves the
/// connection thread running until the app exits; `shutdown` ends it explicitly.
#[derive(Clone)]
pub struct DaemonClient {
    inner: Arc<Inner>,
}

impl DaemonClient {
    /// Starts the connection thread. The returned receiver carries every notice; the
    /// caller must keep draining it or events are lost.
    pub fn spawn(
        transport: Arc<dyn Transport>,
        timeout: Duration,
    ) -> (Self, mpsc::UnboundedReceiver<Notice>) {
        let endpoint = transport.endpoint();
        let link = Arc::new(Mutex::new(LinkState::connecting(&endpoint)));
        let (calls_tx, calls_rx) = mpsc::unbounded_channel();
        let (notices_tx, notices_rx) = mpsc::unbounded_channel();

        let worker_link = Arc::clone(&link);
        let worker_notices = notices_tx.clone();
        let worker_endpoint = endpoint.clone();
        let spawned = std::thread::Builder::new()
            .name("mystique-daemon-client".to_owned())
            .spawn(move || {
                match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime.block_on(supervisor::run(
                        transport,
                        calls_rx,
                        worker_notices,
                        worker_link,
                        timeout,
                    )),
                    Err(error) => {
                        // Without a runtime the client cannot reconnect at all; say so once
                        // and let every request fail through the closed channel.
                        let state = LinkState::disconnected(
                            worker_endpoint,
                            format!("the client runtime could not start: {error}"),
                        );
                        *worker_link.lock().unwrap_or_else(PoisonError::into_inner) = state.clone();
                        let _ = worker_notices.send(Notice::Link(state));
                    }
                }
            });

        if let Err(error) = spawned {
            let _ = notices_tx.send(Notice::Fault(CommandError::internal(format!(
                "the daemon client thread could not start: {error}"
            ))));
        }

        let client = Self {
            inner: Arc::new(Inner {
                calls: calls_tx,
                notices: notices_tx,
                next_id: AtomicU64::new(0),
                timeout,
                link,
            }),
        };
        (client, notices_rx)
    }

    /// The last published link state. Used by `daemon_info`, not by rendering.
    pub fn link(&self) -> LinkState {
        self.inner
            .link
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Sends one command and waits for the daemon's answer.
    pub async fn request(&self, cmd: &str, args: Value) -> Result<Response, CommandError> {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (reply, answer) = oneshot::channel();
        let deadline = std::time::Instant::now() + self.inner.timeout * 2;

        self.inner
            .calls
            .send(Call::Request {
                id,
                cmd: cmd.to_owned(),
                args,
                deadline,
                reply,
            })
            .map_err(|_| {
                daemon_unavailable("the daemon client is not running; restart mystique-ui")
            })?;

        match tokio::time::timeout(self.inner.timeout, answer).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(daemon_unavailable(
                "the daemon connection closed before it answered",
            )),
            Err(_) => {
                let error = daemon_timeout(self.inner.timeout);
                let _ = self.inner.notices.send(Notice::Fault(error.clone()));
                Err(error)
            }
        }
    }

    /// Forces an immediate connection attempt instead of waiting out the backoff.
    pub fn reconnect(&self) {
        let _ = self.inner.calls.send(Call::Reconnect);
    }

    /// Re-publishes the current link without touching the connection. The UI calls this
    /// after a failed spawn so the reason reaches the Connection section.
    pub fn announce(&self) {
        let _ = self.inner.notices.send(Notice::Link(self.link()));
    }

    pub fn shutdown(&self) {
        let _ = self.inner.calls.send(Call::Shutdown);
    }
}
