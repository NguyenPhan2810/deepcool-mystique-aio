//! `0x14` — erase the stored image or GIF.
//!
//! One command serves both UI entry points: the Return to dashboard section and the
//! standalone erase action in Image upload. It is destructive, has no read-back, and is
//! the verified route from stored media back to machine info.

use mystique_ipc::{cmd, CommandError, CommandOutcome};
use serde_json::json;
use tauri::State;

use super::call_data;
use crate::app::AppState;

#[tauri::command]
pub async fn erase_images(state: State<'_, AppState>) -> Result<CommandOutcome, CommandError> {
    call_data(&state.client, cmd::ERASE_IMAGES, json!({})).await
}
