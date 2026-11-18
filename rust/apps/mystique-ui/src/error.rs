//! The client-side error constructors, on top of the shared IPC error shape.
//!
//! The daemon answers every request with `{ code, message, hint, packets }`, and this app
//! passes that object through unchanged, so the frontend keeps one renderer. The codes
//! below exist only on this side of the pipe: the daemon is absent, a request was never
//! answered, or an `sc.exe` admin action failed.

use std::time::Duration;

pub use mystique_ipc::CommandError;

/// The control endpoint is not there, or nothing is listening on it.
pub const DAEMON_UNAVAILABLE: &str = "daemon_unavailable";
/// The request was written but no answer arrived before the deadline.
pub const DAEMON_TIMEOUT: &str = "daemon_timeout";

pub fn daemon_unavailable(detail: impl Into<String>) -> CommandError {
    CommandError::new(DAEMON_UNAVAILABLE, detail)
        .with_hint("start mystique-daemon, or press Reconnect daemon in the Connection section")
}

pub fn daemon_timeout(after: Duration) -> CommandError {
    CommandError::new(
        DAEMON_TIMEOUT,
        format!(
            "the daemon did not answer within {} s",
            after.as_secs_f32().max(0.1)
        ),
    )
    .with_hint(
        "the client dropped the connection and will reconnect; the daemon may be blocked on \
         the panel",
    )
}

/// A UI-side admin action (`sc.exe`) with the backend's verbatim text.
pub fn admin(code: &str, message: impl Into<String>) -> CommandError {
    CommandError::new(code, message)
}
