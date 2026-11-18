//! The bridge between the daemon client and the webview.
//!
//! One task owns the notice stream. It keeps the last link state, the last device
//! snapshot and the last fault, so every emitted message carries a classified
//! [`Health`] rather than raw booleans the JavaScript would have to interpret.
//!
//! # Reconnect
//!
//! Events are not replayed by the daemon. Whenever the link goes from not-connected to
//! connected, this task asks for `device_state` and then `status` before it renders
//! anything that arrived on the new connection. Notices are processed in order, so a
//! frame that arrives during the resync is rendered after it.

use mystique_ipc::{cmd, CommandError, DeviceState, StatusDto};
use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

use crate::daemon::{health, DaemonClient, Health, HealthInputs, LinkState, Notice, SpawnRecord};

pub const CONNECTION: &str = "daemon-connection";
pub const STATE: &str = "daemon-state";
pub const STATUS: &str = "daemon-status";
pub const TELEMETRY: &str = "daemon-telemetry";

/// What the Connection section renders. One object, one classification: the frontend
/// never derives "daemon not running" from a combination of flags.
#[derive(Debug, Clone, Serialize)]
pub struct ConnectionPayload {
    /// `connecting`, `connected` or `disconnected`.
    pub phase: &'static str,
    pub endpoint: String,
    /// True only when this window started the daemon. It says where the process came
    /// from; it never implies that closing this window stops it.
    pub spawned_by_ui: bool,
    /// Why this window failed to start the daemon, if it tried and failed.
    pub launch_error: Option<String>,
    pub health: Health,
}

pub fn connection_payload(
    client: &DaemonClient,
    spawn: &SpawnRecord,
    device: Option<&DeviceState>,
    fault: Option<&CommandError>,
) -> ConnectionPayload {
    let link = client.link();
    let launch_error = spawn.last_error();
    let health = health(HealthInputs {
        link: &link,
        device,
        fault,
        launch_error: launch_error.as_deref(),
    });
    ConnectionPayload {
        phase: phase(&link),
        endpoint: link.endpoint().to_owned(),
        spawned_by_ui: spawn.spawned_by_ui(),
        launch_error,
        health,
    }
}

fn phase(link: &LinkState) -> &'static str {
    match link {
        LinkState::Connecting { .. } => "connecting",
        LinkState::Connected { .. } => "connected",
        LinkState::Disconnected { .. } => "disconnected",
    }
}

/// Starts the forwarding task. The notice receiver must be the one `DaemonClient::spawn`
/// returned; all events after this point reach the window.
pub fn spawn(
    app: AppHandle,
    client: DaemonClient,
    spawn: SpawnRecord,
    notices: mpsc::UnboundedReceiver<Notice>,
) {
    let bridge = Bridge {
        app,
        client,
        spawn,
        link: LinkState::connecting(""),
        device: None,
        fault: None,
    };
    tauri::async_runtime::spawn(bridge.run(notices));
}

struct Bridge {
    app: AppHandle,
    client: DaemonClient,
    spawn: SpawnRecord,
    link: LinkState,
    device: Option<DeviceState>,
    fault: Option<CommandError>,
}

impl Bridge {
    async fn run(mut self, mut notices: mpsc::UnboundedReceiver<Notice>) {
        self.link = self.client.link();
        self.emit_connection();

        while let Some(notice) = notices.recv().await {
            match notice {
                Notice::Link(link) => {
                    let reconnected = link.is_connected() && !self.link.is_connected();
                    if !link.is_connected() {
                        // A stale device snapshot must not outlive the link that produced
                        // it, or the panel would keep claiming the daemon owns the panel.
                        self.device = None;
                        self.fault = None;
                    }
                    self.link = link;
                    self.emit_connection();
                    if reconnected {
                        self.resync().await;
                    }
                }
                Notice::State(state) => {
                    self.fault = None;
                    self.device = Some((*state).clone());
                    self.emit(STATE, &*state);
                    self.emit_connection();
                }
                Notice::Telemetry(frame) => {
                    let cleared = self.fault.take().is_some();
                    self.emit(TELEMETRY, &frame);
                    if cleared {
                        self.emit_connection();
                    }
                }
                Notice::Fault(fault) => {
                    self.fault = Some(fault);
                    self.emit_connection();
                }
            }
        }
    }

    /// Catches up after a connection transition: the snapshot first, the advisory
    /// `0x10` read second, then ordinary events.
    async fn resync(&mut self) {
        match self.client.request(cmd::DEVICE_STATE, json!({})).await {
            Ok(response) => match response.into_data::<DeviceState>() {
                Ok(state) => {
                    self.fault = None;
                    self.device = Some(state.clone());
                    self.emit(STATE, &state);
                }
                Err(error) => self.fault = Some(error),
            },
            Err(error) => self.fault = Some(error),
        }

        match self.client.request(cmd::STATUS, json!({})).await {
            Ok(response) => match response.into_data::<StatusDto>() {
                Ok(status) => {
                    self.fault = None;
                    self.device = Some(status.device.clone());
                    self.emit(STATUS, &status);
                }
                Err(error) => self.fault = Some(error),
            },
            Err(error) => self.fault = Some(error),
        }

        self.emit_connection();
    }

    fn emit_connection(&self) {
        let payload = connection_payload(
            &self.client,
            &self.spawn,
            self.device.as_ref(),
            self.fault.as_ref(),
        );
        self.emit(CONNECTION, &payload);
    }

    fn emit<T: Serialize + Clone>(&self, event: &str, payload: &T) {
        if let Err(error) = self.app.emit(event, payload.clone()) {
            // The window closing is not an error worth interrupting the stream for, but
            // it must not be silent either.
            eprintln!("mystique-ui: could not emit {event}: {error}");
        }
    }
}
