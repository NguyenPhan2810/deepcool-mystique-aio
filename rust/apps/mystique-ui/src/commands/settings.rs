//! Settings commands. Each one is a single request; the daemon owns the session state
//! that makes `0x02` absolute (rotation, gyro, brightness) and remembers the value Screen
//! On restores.

use mystique_ipc::{cmd, CommandError, CommandOutcome};
use serde_json::json;
use tauri::State;

use super::call_data;
use crate::app::AppState;

#[tauri::command]
pub async fn set_mode(
    state: State<'_, AppState>,
    mode: u8,
) -> Result<CommandOutcome, CommandError> {
    call_data(&state.client, cmd::SET_MODE, json!({ "mode": mode })).await
}

#[tauri::command]
pub async fn set_fan_io(
    state: State<'_, AppState>,
    cpu: String,
    pump: String,
) -> Result<CommandOutcome, CommandError> {
    call_data(
        &state.client,
        cmd::SET_FAN_IO,
        json!({ "cpu": cpu, "pump": pump }),
    )
    .await
}

#[tauri::command]
pub async fn set_rgb_mode(
    state: State<'_, AppState>,
    mode: String,
) -> Result<CommandOutcome, CommandError> {
    call_data(&state.client, cmd::SET_RGB_MODE, json!({ "mode": mode })).await
}

#[tauri::command]
pub async fn set_brightness(
    state: State<'_, AppState>,
    brightness: u8,
) -> Result<CommandOutcome, CommandError> {
    call_data(
        &state.client,
        cmd::SET_BRIGHTNESS,
        json!({ "brightness": brightness }),
    )
    .await
}

#[tauri::command]
pub async fn set_rotation(
    state: State<'_, AppState>,
    rotation: u8,
) -> Result<CommandOutcome, CommandError> {
    call_data(
        &state.client,
        cmd::SET_ROTATION,
        json!({ "rotation": rotation }),
    )
    .await
}

#[tauri::command]
pub async fn set_gyro(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<CommandOutcome, CommandError> {
    call_data(&state.client, cmd::SET_GYRO, json!({ "enabled": enabled })).await
}

#[tauri::command]
pub async fn set_idle_screen(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<CommandOutcome, CommandError> {
    call_data(
        &state.client,
        cmd::SET_IDLE_SCREEN,
        json!({ "enabled": enabled }),
    )
    .await
}

#[tauri::command]
pub async fn set_rgb_sync(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<CommandOutcome, CommandError> {
    call_data(
        &state.client,
        cmd::SET_RGB_SYNC,
        json!({ "enabled": enabled }),
    )
    .await
}

#[tauri::command]
pub async fn set_recording(
    state: State<'_, AppState>,
    cpu_clock: bool,
    cpu_temp: bool,
) -> Result<CommandOutcome, CommandError> {
    call_data(
        &state.client,
        cmd::SET_RECORDING,
        json!({ "cpu_clock": cpu_clock, "cpu_temp": cpu_temp }),
    )
    .await
}

/// The JS-facing name is unchanged; the daemon calls the same operation `set_screen`
/// with `enabled`. Off is brightness 0, not a separate power bit.
#[tauri::command]
pub async fn set_screen_power(
    state: State<'_, AppState>,
    lit: bool,
) -> Result<CommandOutcome, CommandError> {
    call_data(&state.client, cmd::SET_SCREEN, json!({ "enabled": lit })).await
}

#[tauri::command]
pub async fn set_areas(
    state: State<'_, AppState>,
    main: u8,
    style: u8,
    aux: u8,
) -> Result<CommandOutcome, CommandError> {
    call_data(
        &state.client,
        cmd::SET_AREAS,
        json!({ "main": main, "style": style, "aux": aux }),
    )
    .await
}

#[tauri::command]
pub async fn set_media_timing(
    state: State<'_, AppState>,
    time: u8,
    animation: u8,
) -> Result<CommandOutcome, CommandError> {
    call_data(
        &state.client,
        cmd::SET_MEDIA_TIMING,
        json!({ "time": time, "animation": animation }),
    )
    .await
}

#[tauri::command]
pub async fn set_labels(
    state: State<'_, AppState>,
    cpu: Option<String>,
    gpu: Option<String>,
) -> Result<CommandOutcome, CommandError> {
    call_data(
        &state.client,
        cmd::SET_LABELS,
        json!({ "cpu": cpu, "gpu": gpu }),
    )
    .await
}
