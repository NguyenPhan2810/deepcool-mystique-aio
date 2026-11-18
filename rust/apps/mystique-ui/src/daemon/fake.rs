//! A scripted daemon on a real in-memory duplex pair.
//!
//! The fake is a real `tokio::io` stream, so the supervisor's framing, id matching and
//! reconnect logic run unmodified. Each connection is answered by a closure that can
//! push events, answer out of order, send a stray id, or stay silent; `closing_after`
//! drops a connection on demand.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mystique_ipc::{decode, encode, Request, Response, ServerMessage};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use super::transport::{Connecting, Stream, Transport, TransportError};
use super::{DaemonClient, Notice};

type Answer = Box<dyn FnMut(&Request, usize) -> Vec<ServerMessage> + Send>;

pub(crate) struct FakeDaemon {
    present: Arc<AtomicBool>,
    connections: Arc<AtomicUsize>,
    received: Arc<Mutex<Vec<Request>>>,
    answer: Arc<Mutex<Answer>>,
    greeting: Arc<Mutex<Vec<ServerMessage>>>,
    close_after: Arc<Mutex<Vec<usize>>>,
}

impl Clone for FakeDaemon {
    fn clone(&self) -> Self {
        Self {
            present: Arc::clone(&self.present),
            connections: Arc::clone(&self.connections),
            received: Arc::clone(&self.received),
            answer: Arc::clone(&self.answer),
            greeting: Arc::clone(&self.greeting),
            close_after: Arc::clone(&self.close_after),
        }
    }
}

impl FakeDaemon {
    pub fn new(answer: impl FnMut(&Request, usize) -> Vec<ServerMessage> + Send + 'static) -> Self {
        Self {
            present: Arc::new(AtomicBool::new(true)),
            connections: Arc::new(AtomicUsize::new(0)),
            received: Arc::new(Mutex::new(Vec::new())),
            answer: Arc::new(Mutex::new(Box::new(answer))),
            greeting: Arc::new(Mutex::new(Vec::new())),
            close_after: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn absent() -> Self {
        let fake = Self::new(|_, _| Vec::new());
        fake.present.store(false, Ordering::SeqCst);
        fake
    }

    pub fn greeting(self, messages: Vec<ServerMessage>) -> Self {
        *self.greeting.lock().expect("fake greeting") = messages;
        self
    }

    /// Close connection `index` after `requests` answered requests.
    pub fn closing_after(self, index: usize, requests: usize) -> Self {
        {
            let mut limits = self.close_after.lock().expect("fake close limit");
            while limits.len() <= index {
                limits.push(usize::MAX);
            }
            limits[index] = requests;
        }
        self
    }

    pub fn requests(&self) -> Vec<Request> {
        self.received.lock().expect("fake received").clone()
    }

    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }
}

impl Transport for FakeDaemon {
    fn connect(&self, _timeout: Duration) -> Connecting {
        let present = Arc::clone(&self.present);
        let connections = Arc::clone(&self.connections);
        let received = Arc::clone(&self.received);
        let answer = Arc::clone(&self.answer);
        let greeting = Arc::clone(&self.greeting);
        let close_after = Arc::clone(&self.close_after);

        Box::pin(async move {
            if !present.load(Ordering::SeqCst) {
                return Err(TransportError::Absent);
            }
            let index = connections.fetch_add(1, Ordering::SeqCst);
            let (ours, peer) = tokio::io::duplex(64 * 1024);

            tokio::spawn(async move {
                let (read, mut write) = tokio::io::split(peer);
                let mut lines = BufReader::new(read).lines();

                let greeting = greeting.lock().expect("fake greeting").clone();
                for message in greeting {
                    let Ok(line) = encode(&message) else { return };
                    if write.write_all(&line).await.is_err() {
                        return;
                    }
                }

                let limit = close_after
                    .lock()
                    .ok()
                    .and_then(|limits| limits.get(index).copied())
                    .unwrap_or(usize::MAX);
                let mut answered = 0usize;

                while let Ok(Some(line)) = lines.next_line().await {
                    let Ok(request) = decode::<Request>(line.as_bytes()) else {
                        return;
                    };
                    received
                        .lock()
                        .expect("fake received")
                        .push(request.clone());
                    let replies = {
                        let mut answer = answer.lock().expect("fake answer");
                        answer(&request, index)
                    };
                    for reply in replies {
                        let Ok(line) = encode(&reply) else { return };
                        if write.write_all(&line).await.is_err() {
                            return;
                        }
                    }
                    answered += 1;
                    if answered >= limit {
                        return;
                    }
                }
            });

            Ok(Box::new(ours) as Box<dyn Stream>)
        })
    }

    fn is_available(&self) -> bool {
        self.present.load(Ordering::SeqCst)
    }

    fn endpoint(&self) -> String {
        "fake://mystique-control-v1".to_owned()
    }
}

pub fn start(
    fake: &FakeDaemon,
    timeout: Duration,
) -> (DaemonClient, mpsc::UnboundedReceiver<Notice>) {
    DaemonClient::spawn(Arc::new(fake.clone()), timeout)
}

pub fn answer_with(data: Value) -> impl FnMut(&Request, usize) -> Vec<ServerMessage> + Send {
    move |request: &Request, _| {
        vec![ServerMessage::Response(
            Response::success(request.id, &data).expect("a serialisable test payload"),
        )]
    }
}
