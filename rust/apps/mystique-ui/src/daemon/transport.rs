//! The byte stream this client talks over, and the trait the tests fake.
//!
//! Everything above this module sees a `Transport`: open one connection, read lines,
//! write lines. Nothing here knows what a request or an event is. The daemon owns the
//! real endpoint, so a failed connect is an ordinary state, not a crash.
//!
//! # Endpoint
//!
//! Windows: `\\.\pipe\mystique-control-v1` (the daemon creates one instance per client).
//! Unix: `$XDG_RUNTIME_DIR/mystique/control.sock`, falling back to
//! `/tmp/mystique-<uid>/control.sock`. `MYSTIQUE_ENDPOINT` overrides both, and the child
//! process is started with the same value, so a development pair always agrees.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite};

/// Anything the supervisor can read and write lines on. Implemented by the platform
/// stream and by tokio's in-memory duplex pair in the tests.
pub trait Stream: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T> Stream for T where T: AsyncRead + AsyncWrite + Send + Unpin {}

/// Why a connection could not be opened. `Absent` is the normal "daemon not running"
/// case and is never retried differently from any other failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// Nothing is listening on the endpoint.
    Absent,
    /// Something is there but this user may not open it.
    Denied,
    /// Anything else: a busy pipe, a broken socket, a timeout.
    Broken(String),
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Absent => formatter.write_str("nothing is listening on the control endpoint"),
            Self::Denied => formatter.write_str("the control endpoint denied access to this user"),
            Self::Broken(detail) => formatter.write_str(detail),
        }
    }
}

/// The future a `connect` returns. Boxed so `Transport` stays dyn-compatible: the
/// supervisor holds a `Box<dyn Transport>` and does not care which platform it is.
pub type Connecting = Pin<Box<dyn Future<Output = Result<Box<dyn Stream>, TransportError>> + Send>>;

pub trait Transport: Send + Sync + 'static {
    /// Opens one connection. The supervisor calls this again after every disconnect.
    fn connect(&self, timeout: Duration) -> Connecting;

    /// A cheap, non-blocking check for "is anything listening". Used only to decide
    /// whether to start a daemon; a false positive costs one refused connection, and a
    /// false negative costs one redundant spawn that the daemon's own lock rejects.
    fn is_available(&self) -> bool;

    /// The endpoint in the form the UI shows.
    fn endpoint(&self) -> String;
}

fn classify(error: &io::Error) -> TransportError {
    match error.kind() {
        io::ErrorKind::NotFound => TransportError::Absent,
        io::ErrorKind::PermissionDenied => TransportError::Denied,
        _ => TransportError::Broken(error.to_string()),
    }
}

/// The real endpoint for this platform.
pub fn platform() -> Result<Box<dyn Transport>, String> {
    platform::transport()
}

#[cfg(windows)]
mod platform {
    use std::time::Duration;

    use tokio::net::windows::named_pipe::ClientOptions;

    use super::{classify, Connecting, Stream, Transport};

    /// The documented pipe, and what a bare `MYSTIQUE_ENDPOINT` expands to.
    pub const DEFAULT_ENDPOINT: &str = r"\\.\pipe\mystique-control-v1";

    pub struct NamedPipeTransport {
        pipe: String,
    }

    #[cfg(test)]
    impl NamedPipeTransport {
        pub fn at(pipe: impl Into<String>) -> Self {
            Self { pipe: pipe.into() }
        }
    }

    impl Transport for NamedPipeTransport {
        fn connect(&self, _timeout: Duration) -> Connecting {
            let pipe = self.pipe.clone();
            Box::pin(async move {
                // `ClientOptions::open` is synchronous, but it does not block on a busy
                // pipe: Windows answers ERROR_PIPE_BUSY immediately. The daemon keeps a
                // spare instance ready, so a refusal is a state to retry, not a wedge.
                match ClientOptions::new().open(&pipe) {
                    Ok(client) => Ok(Box::new(client) as Box<dyn Stream>),
                    Err(error) => Err(classify(&error)),
                }
            })
        }

        fn is_available(&self) -> bool {
            // Windows has no non-destructive "does this pipe exist" probe without a raw
            // Win32 call, so this opens one client and drops it. The daemon creates the
            // next pipe instance *before* it serves a client, so the probe costs one
            // accept cycle and cannot starve a real connect; a busy or denied pipe still
            // means something is there.
            match ClientOptions::new().open(&self.pipe) {
                Ok(_) => true,
                Err(error) => error.kind() != std::io::ErrorKind::NotFound,
            }
        }

        fn endpoint(&self) -> String {
            self.pipe.clone()
        }
    }

    /// `MYSTIQUE_ENDPOINT` may be a full pipe path or a bare pipe name.
    fn resolve() -> String {
        let value = std::env::var("MYSTIQUE_ENDPOINT").unwrap_or_default();
        let value = value.trim();
        if value.is_empty() {
            return DEFAULT_ENDPOINT.to_owned();
        }
        if value.starts_with(r"\\.\pipe\") {
            value.to_owned()
        } else {
            format!(r"\\.\pipe\{value}")
        }
    }

    pub fn transport() -> Result<Box<dyn Transport>, String> {
        Ok(Box::new(NamedPipeTransport { pipe: resolve() }))
    }
}

#[cfg(unix)]
mod platform {
    use std::path::PathBuf;
    use std::time::Duration;

    use super::{classify, Connecting, Stream, Transport, TransportError};

    pub struct UnixTransport {
        path: PathBuf,
    }

    #[cfg(test)]
    impl UnixTransport {
        pub(super) fn at(path: PathBuf) -> Self {
            Self { path }
        }
    }

    impl Transport for UnixTransport {
        fn connect(&self, timeout: Duration) -> Connecting {
            let path = self.path.clone();
            Box::pin(async move {
                match tokio::time::timeout(timeout, tokio::net::UnixStream::connect(&path)).await {
                    Ok(Ok(stream)) => Ok(Box::new(stream) as Box<dyn Stream>),
                    Ok(Err(error)) => Err(classify(&error)),
                    Err(_) => Err(TransportError::Broken(format!(
                        "connecting to {} timed out after {} ms",
                        path.display(),
                        timeout.as_millis()
                    ))),
                }
            })
        }

        fn is_available(&self) -> bool {
            // A stale socket file outlives the daemon that created it, so `exists()` is
            // not a probe: after a crash the file stays and the UI would never spawn a
            // replacement. Only a successful connect proves a listener is there. The
            // connection is accepted and dropped, exactly like the Windows pipe probe.
            std::os::unix::net::UnixStream::connect(&self.path).is_ok()
        }

        fn endpoint(&self) -> String {
            self.path.display().to_string()
        }
    }

    fn control_dir() -> Result<PathBuf, String> {
        if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
            if !dir.is_empty() {
                return Ok(PathBuf::from(dir).join("mystique"));
            }
        }
        if let Some(uid) = current_uid() {
            return Ok(PathBuf::from(format!("/tmp/mystique-{uid}")));
        }
        Err(
            "XDG_RUNTIME_DIR is unset and the uid could not be read; set MYSTIQUE_ENDPOINT"
                .to_owned(),
        )
    }

    #[cfg(target_os = "linux")]
    fn current_uid() -> Option<u32> {
        // No libc and no `unsafe`: /proc/self/status is text with a `Uid:` line.
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        let line = status.lines().find(|line| line.starts_with("Uid:"))?;
        line.split_whitespace().nth(1)?.parse().ok()
    }

    #[cfg(all(unix, not(target_os = "linux")))]
    fn current_uid() -> Option<u32> {
        None
    }

    fn resolve() -> Result<PathBuf, String> {
        let value = std::env::var("MYSTIQUE_ENDPOINT").unwrap_or_default();
        let value = value.trim();
        if value.is_empty() {
            return Ok(control_dir()?.join("control.sock"));
        }
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err(format!(
                "MYSTIQUE_ENDPOINT must be an absolute path, got \"{value}\""
            ));
        }
        Ok(path)
    }

    pub fn transport() -> Result<Box<dyn Transport>, String> {
        Ok(Box::new(UnixTransport { path: resolve()? }))
    }
}

#[cfg(not(any(windows, unix)))]
mod platform {
    use super::Transport;

    pub fn transport() -> Result<Box<dyn Transport>, String> {
        Err("no control transport exists for this platform".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_endpoint_is_absent_and_a_permission_failure_is_denied() {
        assert_eq!(
            classify(&io::Error::from(io::ErrorKind::NotFound)),
            TransportError::Absent
        );
        assert_eq!(
            classify(&io::Error::from(io::ErrorKind::PermissionDenied)),
            TransportError::Denied
        );
        match classify(&io::Error::from(io::ErrorKind::TimedOut)) {
            TransportError::Broken(detail) => assert!(!detail.is_empty()),
            other => panic!("expected a broken transport, got {other:?}"),
        }
    }

    #[cfg(windows)]
    #[test]
    fn the_default_pipe_is_the_daemons() {
        assert_eq!(platform::DEFAULT_ENDPOINT, r"\\.\pipe\mystique-control-v1");
    }

    /// The pipe name and the presence probe are load-bearing: a wrong name looks exactly
    /// like a daemon that is not running. A real pipe server, no device.
    #[cfg(windows)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_real_pipe_accepts_the_client_and_carries_a_line() {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
        use tokio::net::windows::named_pipe::ServerOptions;

        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.subsec_nanos())
            .unwrap_or(0);
        let absent = platform::NamedPipeTransport::at(format!(
            r"\\.\pipe\mystique-ui-absent-{}-{stamp}",
            std::process::id()
        ));
        assert!(!absent.is_available(), "no server means no pipe");

        let name = format!(r"\\.\pipe\mystique-ui-test-{}-{stamp}", std::process::id());
        let transport = platform::NamedPipeTransport::at(name.clone());

        // The probe consumes one instance, exactly as it does against the daemon, which
        // pre-creates the next one before serving. Accept the probe, then create the
        // spare the real connection will use.
        let first = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&name)
            .expect("a test pipe");
        assert!(transport.is_available(), "a live pipe is present");
        first.connect().await.expect("the probe is accepted");
        drop(first);

        let server = ServerOptions::new()
            .create(&name)
            .expect("a spare instance");
        let peer = tokio::spawn(async move {
            server.connect().await.expect("a client");
            let (read, mut write) = tokio::io::split(server);
            let mut lines = BufReader::new(read).lines();
            if let Ok(Some(line)) = lines.next_line().await {
                let _ = write.write_all(format!("{line}\n").as_bytes()).await;
            }
        });

        let mut stream = transport
            .connect(Duration::from_secs(2))
            .await
            .expect("the client opens the pipe");
        stream.write_all(b"ping\n").await.expect("write");
        let mut buffer = [0u8; 16];
        let read = stream.read(&mut buffer).await.expect("read");
        assert_eq!(&buffer[..read], b"ping\n");

        let _ = peer.await;
        drop(stream);
    }

    /// A socket file survives a crashed daemon on Unix. The spawn decision must not be
    /// fooled by it, or the UI never starts a replacement and every request fails.
    #[cfg(unix)]
    #[test]
    fn a_stale_socket_file_does_not_count_as_a_running_daemon() {
        let dir =
            std::env::temp_dir().join(format!("mystique-ui-transport-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("control.sock");

        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        drop(listener);
        assert!(path.exists(), "the socket file outlives its listener");

        let transport = platform::UnixTransport::at(path.clone());
        assert!(!transport.is_available(), "nothing is listening");

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The positive half: a live listener is found by the same probe.
    #[cfg(unix)]
    #[test]
    fn a_live_unix_listener_is_available() {
        let dir = std::env::temp_dir().join(format!("mystique-ui-live-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("control.sock");

        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let transport = platform::UnixTransport::at(path.clone());
        assert!(transport.is_available(), "a bound socket is a listener");
        assert!(transport.endpoint().ends_with("control.sock"));

        drop(listener);
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
