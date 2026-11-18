//! Connection, live status, and the two vendor-service admin actions.
//!
//! `open_device` and `close_device` are now requests to the daemon: this window never
//! holds the handle. `stop_service` and `start_service` stay local `sc.exe` calls because
//! they are an operator decision about the *vendor* service, not device ownership.

use mystique_ipc::{cmd, CommandError, DeviceState, OpenResult, ShutdownAck, StatusDto};
use serde::Serialize;
use serde_json::json;
use tauri::State;

use super::call_data;
use crate::app::AppState;
use crate::error::admin;
use crate::events::ConnectionPayload;
use crate::service;

#[tauri::command]
pub async fn status(state: State<'_, AppState>) -> Result<StatusDto, CommandError> {
    call_data(&state.client, cmd::STATUS, json!({})).await
}

#[tauri::command]
pub async fn device_state(state: State<'_, AppState>) -> Result<DeviceState, CommandError> {
    call_data(&state.client, cmd::DEVICE_STATE, json!({})).await
}

#[tauri::command]
pub async fn open_device(state: State<'_, AppState>) -> Result<OpenResult, CommandError> {
    call_data(&state.client, cmd::OPEN_DEVICE, json!({})).await
}

#[tauri::command]
pub async fn close_device(state: State<'_, AppState>) -> Result<DeviceState, CommandError> {
    call_data(&state.client, cmd::CLOSE_DEVICE, json!({})).await
}

/// The daemon's view, or `None` when it is not running. A `null` here is rendered as
/// "unknown", never as a fabricated closed handle.
async fn snapshot(state: &AppState) -> Option<DeviceState> {
    call_data(&state.client, cmd::DEVICE_STATE, json!({}))
        .await
        .ok()
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceResult {
    /// Verbatim `sc.exe start|stop MystiqueService` output.
    pub output: String,
    /// The daemon's state after the action, or `None` when the daemon is not running.
    pub device: Option<DeviceState>,
}

#[tauri::command]
pub async fn stop_service(state: State<'_, AppState>) -> Result<ServiceResult, CommandError> {
    let output = service::stop().map_err(|message| admin("service_stop_failed", message))?;
    let device = snapshot(state.inner()).await;
    Ok(ServiceResult { output, device })
}

#[tauri::command]
pub async fn start_service(state: State<'_, AppState>) -> Result<ServiceResult, CommandError> {
    let output = service::start().map_err(|message| admin("service_start_failed", message))?;
    let device = snapshot(state.inner()).await;
    Ok(ServiceResult { output, device })
}

/// The connection snapshot the webview applies at boot: phase, endpoint, where the
/// daemon process came from, and the classified health sentence.
#[tauri::command]
pub async fn daemon_info(state: State<'_, AppState>) -> Result<ConnectionPayload, CommandError> {
    Ok(state.inner().connection())
}

/// Starts the daemon if the endpoint is absent, reconnects, and answers with the state to
/// render. The daemon row carries any failure; this is not a section error.
#[tauri::command]
pub async fn daemon_retry(state: State<'_, AppState>) -> Result<ConnectionPayload, CommandError> {
    Ok(state.inner().retry().await)
}

/// Asks the daemon to stop. This is the only path that ends the process: closing the
/// window does not. The daemon acknowledges before it stops, so the caller learns that
/// the request arrived even though it releases the panel and the stream right after.
#[tauri::command]
pub async fn daemon_stop(state: State<'_, AppState>) -> Result<ShutdownAck, CommandError> {
    call_data(&state.client, cmd::SHUTDOWN, json!({})).await
}
