//! Upload preview and send. The plan, the frames and the content id live in the daemon
//! session, so the counts the owner confirms are the counts that are written.

use mystique_ipc::{cmd, CommandError, CommandOutcome, UploadPlanSummary};
use serde_json::json;
use tauri::State;

use super::call_data;
use crate::app::AppState;

#[tauri::command]
pub async fn plan_upload(
    state: State<'_, AppState>,
    path: String,
    fit: Option<String>,
) -> Result<UploadPlanSummary, CommandError> {
    call_data(
        &state.client,
        cmd::PLAN_UPLOAD,
        json!({ "path": path, "fit": fit }),
    )
    .await
}

#[tauri::command]
pub async fn upload_image(
    state: State<'_, AppState>,
    path: String,
    fit: Option<String>,
) -> Result<CommandOutcome, CommandError> {
    call_data(
        &state.client,
        cmd::UPLOAD_IMAGE,
        json!({ "path": path, "fit": fit }),
    )
    .await
}

#[tauri::command]
pub async fn upload_gif(
    state: State<'_, AppState>,
    path: String,
    fit: Option<String>,
) -> Result<CommandOutcome, CommandError> {
    call_data(
        &state.client,
        cmd::UPLOAD_GIF,
        json!({ "path": path, "fit": fit }),
    )
    .await
}
