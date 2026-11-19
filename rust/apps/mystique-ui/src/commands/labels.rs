//! `detected_labels`: the names the daemon found on this host.
//!
//! Detection lives in the daemon (`mystique-sensors`), so the window does not depend on
//! `sysinfo`, PowerShell or `lspci`. This wrapper relays one request and keeps the exact
//! JS-facing name; an empty list is a valid answer, not an error.

use mystique_ipc::{cmd, CommandError, DetectedLabels};
use serde_json::json;
use tauri::State;

use super::call_data;
use crate::app::AppState;

#[tauri::command]
pub async fn detected_labels(state: State<'_, AppState>) -> Result<DetectedLabels, CommandError> {
    call_data(&state.client, cmd::DETECTED_LABELS, json!({})).await
}
