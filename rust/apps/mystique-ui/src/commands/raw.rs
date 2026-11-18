//! The raw packet surface. The daemon builds and checksums the 48-byte request; this
//! command only forwards the owner's command byte, payload and pipe choice.

use mystique_ipc::{cmd, CommandError, RawPacketOutcome};
use serde_json::json;
use tauri::State;

use super::call_data;
use crate::app::AppState;

#[tauri::command]
pub async fn raw_packet(
    state: State<'_, AppState>,
    command: u8,
    payload: Vec<u8>,
    pipe: String,
    dry_run: bool,
) -> Result<RawPacketOutcome, CommandError> {
    call_data(
        &state.client,
        cmd::RAW_PACKET,
        json!({
            "command": command,
            "payload": payload,
            "pipe": pipe,
            "dry_run": dry_run,
        }),
    )
    .await
}
