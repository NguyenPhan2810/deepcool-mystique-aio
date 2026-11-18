//! Start, stop and inspect the daemon's `0x01` stream.
//!
//! The loop lives in the daemon, not here. These commands flip its intent and return the
//! snapshot; frames arrive as `daemon-telemetry` events, so nothing polls.

use mystique_ipc::{cmd, CommandError, DeviceState, TelemetryStatus};
use serde_json::json;
use tauri::State;

use super::call_data;
use crate::app::AppState;

#[tauri::command]
pub async fn start_telemetry(state: State<'_, AppState>) -> Result<TelemetryStatus, CommandError> {
    call_data(&state.client, cmd::START_STREAM, json!({})).await
}

#[tauri::command]
pub async fn stop_telemetry(state: State<'_, AppState>) -> Result<TelemetryStatus, CommandError> {
    call_data(&state.client, cmd::STOP_STREAM, json!({})).await
}

#[tauri::command]
pub async fn set_stream_rate(
    state: State<'_, AppState>,
    hz: u32,
) -> Result<TelemetryStatus, CommandError> {
    call_data(&state.client, cmd::SET_STREAM_RATE, json!({ "hz": hz })).await
}

/// The daemon has no separate `telemetry_status`; the stream's state travels inside
/// `device_state`. This projects it so the frontend keeps one shape.
#[tauri::command]
pub async fn telemetry_status(state: State<'_, AppState>) -> Result<TelemetryStatus, CommandError> {
    let device: DeviceState = call_data(&state.client, cmd::DEVICE_STATE, json!({})).await?;
    device.telemetry.ok_or_else(|| {
        CommandError::internal("the daemon returned a device state without telemetry")
    })
}
