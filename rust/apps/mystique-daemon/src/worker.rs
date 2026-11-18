//! The thread that owns the panel.
//!
//! Every USB operation goes through one [`Core`] on one thread. That is the ownership
//! rule made structural: clients cannot open a second handle, and no two transfers can
//! interleave. Requests are bounded; the async side never blocks on the device, it
//! awaits a oneshot.

use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use mystique_ipc::{codes, CommandError, Event, Request, Response, ServerMessage};
use tokio::sync::{broadcast, oneshot};

use crate::core::Core;

/// Requests in flight. A client that floods gets `busy` rather than unbounded memory.
const REQUEST_QUEUE: usize = 64;
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

enum WorkerMessage {
    Request {
        request: Request,
        reply: oneshot::Sender<Response>,
    },
    Shutdown {
        reply: oneshot::Sender<()>,
    },
}

/// A cloneable handle the per-client tasks use.
#[derive(Clone)]
pub struct WorkerSender {
    tx: SyncSender<WorkerMessage>,
}

impl WorkerSender {
    /// Queues one request. The receiver answers on the returned oneshot.
    pub fn request(&self, request: Request) -> Result<oneshot::Receiver<Response>, CommandError> {
        let (reply, receiver) = oneshot::channel();
        match self.tx.try_send(WorkerMessage::Request { request, reply }) {
            Ok(()) => Ok(receiver),
            Err(TrySendError::Full(_)) => Err(CommandError::new(
                codes::BUSY,
                "the device worker queue is full; retry in a moment",
            )),
            Err(TrySendError::Disconnected(_)) => {
                Err(CommandError::internal("the device worker is not running"))
            }
        }
    }

    fn shutdown(&self) -> Result<oneshot::Receiver<()>, CommandError> {
        let (reply, receiver) = oneshot::channel();
        match self.tx.try_send(WorkerMessage::Shutdown { reply }) {
            Ok(()) => Ok(receiver),
            Err(TrySendError::Full(_)) => Err(CommandError::new(
                codes::BUSY,
                "the device worker queue is full; cannot stop it yet",
            )),
            Err(TrySendError::Disconnected(_)) => {
                Err(CommandError::internal("the device worker is not running"))
            }
        }
    }
}

pub struct Worker {
    sender: WorkerSender,
    _join: JoinHandle<()>,
}

impl Worker {
    pub fn spawn(
        core: Core,
        events: broadcast::Sender<ServerMessage>,
    ) -> Result<Self, std::io::Error> {
        let (tx, rx) = sync_channel(REQUEST_QUEUE);
        let join = thread::Builder::new()
            .name("mystique-device".to_owned())
            .spawn(move || run(core, rx, events))?;
        Ok(Self {
            sender: WorkerSender { tx },
            _join: join,
        })
    }

    pub fn sender(&self) -> WorkerSender {
        self.sender.clone()
    }

    /// Stops the stream, blanks the panel, and waits for the worker to say it is done.
    /// Returns false when the worker was too busy or too wedged to acknowledge.
    pub async fn shutdown(self) -> bool {
        let receiver = match self.sender.shutdown() {
            Ok(receiver) => receiver,
            Err(_) => return false,
        };
        matches!(
            tokio::time::timeout(SHUTDOWN_TIMEOUT, receiver).await,
            Ok(Ok(()))
        )
        // `_join` is dropped here on purpose: a wedged transfer must not stop the
        // process from exiting. Dropping a JoinHandle detaches the thread, and process
        // exit releases the USB interface.
    }
}

fn run(mut core: Core, rx: Receiver<WorkerMessage>, events: broadcast::Sender<ServerMessage>) {
    loop {
        let wait = core.next_wait(Instant::now());
        let message = match wait {
            Some(duration) => match rx.recv_timeout(duration) {
                Ok(message) => Some(message),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match rx.recv() {
                Ok(message) => Some(message),
                Err(_) => break,
            },
        };

        match message {
            Some(WorkerMessage::Request { request, reply }) => {
                let (response, state_events) = core.handle(&request, Instant::now());
                let _ = reply.send(response);
                broadcast(&events, state_events);
            }
            Some(WorkerMessage::Shutdown { reply }) => {
                let shutdown_events = core.shutdown();
                broadcast(&events, shutdown_events);
                let _ = reply.send(());
                return;
            }
            None => {}
        }

        broadcast(&events, core.tick(Instant::now()));
    }
    // The client side vanished without a Shutdown. Leave the panel dark if we can.
    broadcast(&events, core.shutdown());
}

fn broadcast(sender: &broadcast::Sender<ServerMessage>, events: Vec<Event>) {
    for event in events {
        // No subscribers is normal: a daemon with no UI attached still runs.
        let _ = sender.send(ServerMessage::Event(event));
    }
}
