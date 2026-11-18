//! The window's own state: the daemon client, the launcher, and the result of trying to
//! start the daemon. There is no device session here by design.

use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::mpsc;

use crate::daemon::{
    platform, DaemonClient, Launcher, Notice, SpawnRecord, Transport, STARTUP_WAIT,
};
use crate::events::{connection_payload, ConnectionPayload};

/// Everything a Tauri command may need. Cloning is cheap; the clones share one client and
/// one child handle.
#[derive(Clone)]
pub struct AppState {
    pub client: DaemonClient,
    pub launcher: Launcher,
    pub spawn: SpawnRecord,
    transport: Arc<dyn Transport>,
    notices: Arc<Mutex<Option<mpsc::UnboundedReceiver<Notice>>>>,
}

/// The three pieces `main` needs from one construction point, so the window cannot
/// accidentally create a second transport that disagrees with the client.
pub struct ClientParts {
    pub client: DaemonClient,
    pub notices: mpsc::UnboundedReceiver<Notice>,
    pub transport: Arc<dyn Transport>,
}

impl AppState {
    pub fn new(
        client: DaemonClient,
        launcher: Launcher,
        spawn: SpawnRecord,
        transport: Arc<dyn Transport>,
        notices: mpsc::UnboundedReceiver<Notice>,
    ) -> Self {
        Self {
            client,
            launcher,
            spawn,
            transport,
            notices: Arc::new(Mutex::new(Some(notices))),
        }
    }

    /// Builds the real client for this platform.
    pub fn connect() -> Result<ClientParts, String> {
        let transport: Arc<dyn Transport> = Arc::from(platform()?);
        let (client, notices) =
            DaemonClient::spawn(Arc::clone(&transport), crate::daemon::REQUEST_TIMEOUT);
        Ok(ClientParts {
            client,
            notices,
            transport,
        })
    }

    /// The event bridge takes the receiver once, during setup.
    pub fn take_notices(&self) -> Option<mpsc::UnboundedReceiver<Notice>> {
        self.notices
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    /// Starts the daemon when the endpoint is absent, then asks the client to reconnect.
    /// Blocking: the caller runs it off the UI thread.
    fn launch(&self) {
        // A healthy link needs no probe: the Windows presence check opens and drops a
        // pipe instance, which is pointless while we are already talking to the daemon.
        if !self.client.link().is_connected() {
            let transport = Arc::clone(&self.transport);
            let available = move || transport.is_available();
            let _ = self.spawn.ensure(&self.launcher, &available, STARTUP_WAIT);
        }
        self.client.reconnect();
        // A successful spawn produces a real Connecting/Connected transition. A failed
        // one may not change the link at all, so push the current state once: the spawn
        // reason is read from the spawn record when the payload is built.
        if self.spawn.last_error().is_some() {
            self.client.announce();
        }
    }

    /// Boot path. Does not block the Tauri setup hook while a child starts.
    pub async fn boot(&self) {
        let state = self.clone();
        let _ = tokio::task::spawn_blocking(move || state.launch()).await;
    }

    /// Reconnect button: start if absent, reconnect, and return what to render.
    pub async fn retry(&self) -> ConnectionPayload {
        let state = self.clone();
        let _ = tokio::task::spawn_blocking(move || state.launch()).await;
        self.connection()
    }

    pub fn connection(&self) -> ConnectionPayload {
        connection_payload(&self.client, &self.spawn, None, None)
    }

    /// Ends the client thread and drops the handle to a daemon this window started. The
    /// daemon itself keeps running, keeps the panel, and keeps streaming.
    pub fn shutdown(&self) {
        self.client.shutdown();
        self.spawn.release();
    }
}
