//! One classification of "what is wrong right now", so the panel never has to guess.
//!
//! The daemon can be absent, connected with no panel, connected with a panel, or
//! unreachable after a request. Each becomes one [`Health`] with a stable code, one
//! sentence of headline and one of detail. The frontend renders these verbatim; it does
//! not infer state from a collection of booleans.
//!
//! The list is deliberately closed. A new failure mode is added here, with a test, not
//! in the JavaScript.

use serde::Serialize;

use mystique_ipc::{CommandError, DeviceState};

use super::LinkState;
use crate::error::DAEMON_TIMEOUT;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Health {
    /// `ok`, `connecting`, `waiting`, `daemon_unavailable`, `daemon_timeout`,
    /// `device_absent` or `vendor_conflict`.
    pub code: &'static str,
    pub headline: String,
    pub detail: String,
    /// The impossible combination: the daemon says the device is open while the vendor
    /// service also says it is running.
    pub conflict: bool,
}

/// Everything the classification may look at. All fields are cached copies of the last
/// thing the daemon said; nothing here performs I/O.
pub struct Inputs<'a> {
    pub link: &'a LinkState,
    pub device: Option<&'a DeviceState>,
    pub fault: Option<&'a CommandError>,
    /// Set when this window tried to start the daemon and could not.
    pub launch_error: Option<&'a str>,
}

pub fn health(inputs: Inputs<'_>) -> Health {
    match inputs.link {
        LinkState::Connecting { endpoint } => Health {
            code: "connecting",
            headline: "Contacting the daemon".to_owned(),
            detail: format!("Opening {endpoint}. The panel is not touched until it answers."),
            conflict: false,
        },
        LinkState::Disconnected { endpoint, reason } => {
            let detail = match inputs.launch_error {
                Some(error) => format!(
                    "Nothing is listening on {endpoint}, and the daemon could not be started: \
                     {error} Start mystique-daemon yourself, or fix that and press Reconnect."
                ),
                None => format!(
                    "{endpoint} is not there ({reason}). The panel is free; no command can run \
                     until a daemon owns it."
                ),
            };
            Health {
                code: "daemon_unavailable",
                headline: "Daemon not running".to_owned(),
                detail,
                conflict: false,
            }
        }
        LinkState::Connected { endpoint } => {
            if let Some(fault) = inputs.fault.filter(|fault| fault.code == DAEMON_TIMEOUT) {
                return Health {
                    code: "daemon_timeout",
                    headline: "The daemon did not answer".to_owned(),
                    detail: format!(
                        "{} The client dropped {endpoint} and is reconnecting; a write in \
                         progress may still be on the wire.",
                        fault.message
                    ),
                    conflict: false,
                };
            }
            let Some(device) = inputs.device else {
                return Health {
                    code: "waiting",
                    headline: "Reading the daemon's state".to_owned(),
                    detail: "The control socket is open; the panel state has not arrived yet."
                        .to_owned(),
                    conflict: false,
                };
            };
            if !device.device_present {
                return Health {
                    code: "device_absent",
                    headline: "Panel not found".to_owned(),
                    detail: "The daemon is running and retrying by itself. No MYSTIQUE panel \
                             is on the bus (3633:0009); plug it in and the stream resumes."
                        .to_owned(),
                    conflict: false,
                };
            }
            if device.device_open && device.service_running {
                return Health {
                    code: "vendor_conflict",
                    headline: "Two owners".to_owned(),
                    detail: "The daemon reports an open device while MystiqueService also \
                             reports RUNNING. The USB interface admits one owner, so this \
                             should be impossible; stop the vendor service before trusting \
                             any write."
                        .to_owned(),
                    conflict: true,
                };
            }
            if device.device_open {
                Health {
                    code: "ok",
                    headline: "Daemon owns the panel".to_owned(),
                    detail: "Commands, uploads and telemetry all go through the daemon. \
                             Closing this window does not release the panel."
                        .to_owned(),
                    conflict: false,
                }
            } else {
                Health {
                    code: "ok",
                    headline: "Daemon ready".to_owned(),
                    detail: "The daemon is connected and the panel is released. Open it in \
                             daemon to send anything."
                        .to_owned(),
                    conflict: false,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mystique_ipc::SettingsState;

    fn device(present: bool, open: bool, service: bool) -> DeviceState {
        DeviceState {
            device_open: open,
            device_present: present,
            service_running: service,
            service_supported: true,
            service_output: String::new(),
            last_error: None,
            rotation: None,
            settings: SettingsState::default(),
            telemetry: None,
        }
    }

    fn link() -> LinkState {
        LinkState::connected(r"\\.\pipe\mystique-control-v1")
    }

    #[test]
    fn a_missing_endpoint_is_the_daemon_not_running_state() {
        let link = LinkState::disconnected(r"\\.\pipe\mystique-control-v1", "not found");
        let result = health(Inputs {
            link: &link,
            device: None,
            fault: None,
            launch_error: None,
        });
        assert_eq!(result.code, "daemon_unavailable");
        assert!(result.detail.contains("not there"));
        assert!(!result.conflict);
    }

    #[test]
    fn a_failed_spawn_names_its_reason_in_the_same_state() {
        let link = LinkState::disconnected(r"\\.\pipe\mystique-control-v1", "not found");
        let result = health(Inputs {
            link: &link,
            device: None,
            fault: None,
            launch_error: Some("could not start mystique-daemon.exe: os error 2"),
        });
        assert_eq!(result.code, "daemon_unavailable");
        assert!(result.detail.contains("could not be started"));
        assert!(result.detail.contains("os error 2"));
    }

    #[test]
    fn a_running_daemon_with_no_panel_says_so_without_blaming_the_client() {
        let link = link();
        let absent = device(false, false, false);
        let result = health(Inputs {
            link: &link,
            device: Some(&absent),
            fault: None,
            launch_error: None,
        });
        assert_eq!(result.code, "device_absent");
        assert!(result.detail.contains("retrying"));
    }

    #[test]
    fn an_open_device_plus_a_running_vendor_service_is_reported_as_a_conflict() {
        let link = link();
        let both = device(true, true, true);
        let result = health(Inputs {
            link: &link,
            device: Some(&both),
            fault: None,
            launch_error: None,
        });
        assert_eq!(result.code, "vendor_conflict");
        assert!(result.conflict, "the UI must show the impossible state");
    }

    #[test]
    fn a_request_timeout_outranks_a_stale_device_snapshot() {
        let link = link();
        let open = device(true, true, false);
        let fault = CommandError::new(DAEMON_TIMEOUT, "the daemon did not answer within 10 s");
        let result = health(Inputs {
            link: &link,
            device: Some(&open),
            fault: Some(&fault),
            launch_error: None,
        });
        assert_eq!(result.code, "daemon_timeout");
        assert!(result.detail.contains("did not answer"));
    }

    #[test]
    fn a_connected_daemon_before_the_first_snapshot_is_only_waiting() {
        let link = link();
        let result = health(Inputs {
            link: &link,
            device: None,
            fault: None,
            launch_error: None,
        });
        assert_eq!(result.code, "waiting");
        assert!(!result.detail.is_empty());
    }

    #[test]
    fn the_healthy_states_differ_on_whether_the_panel_is_open() {
        let link = link();
        let open = device(true, true, false);
        let released = device(true, false, false);
        let open_health = health(Inputs {
            link: &link,
            device: Some(&open),
            fault: None,
            launch_error: None,
        });
        let released_health = health(Inputs {
            link: &link,
            device: Some(&released),
            fault: None,
            launch_error: None,
        });
        assert_eq!(open_health.code, "ok");
        assert_eq!(released_health.code, "ok");
        assert!(open_health.detail.contains("Closing this window"));
        assert!(released_health.detail.contains("released"));
    }

    #[test]
    fn a_connecting_link_never_claims_ownership() {
        let link = LinkState::connecting(r"\\.\pipe\mystique-control-v1");
        let result = health(Inputs {
            link: &link,
            device: None,
            fault: None,
            launch_error: None,
        });
        assert_eq!(result.code, "connecting");
        assert!(result.detail.contains("not touched"));
    }
}
