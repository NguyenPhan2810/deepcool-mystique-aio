//! The Tauri command surface: one daemon request per command.
//!
//! Nothing here opens USB, samples the host, or keeps a session. The daemon answers with
//! the same DTOs the frontend already renders, and its `CommandError` is passed through
//! unchanged, so the panel keeps one error renderer.
//!
//! The submodules are public because `tauri::generate_handler!` resolves a command's
//! generated symbols in the module where `#[tauri::command]` was applied.

pub mod connection;
pub mod erase;
pub mod labels;
pub mod raw;
pub mod settings;
pub mod startup;
pub mod telemetry;
pub mod upload;

use mystique_ipc::{CommandError, Response};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::daemon::DaemonClient;

/// Sends one request and returns the envelope, error and all.
pub(crate) async fn call(
    client: &DaemonClient,
    command: &str,
    args: Value,
) -> Result<Response, CommandError> {
    client.request(command, args).await
}

/// Sends one request and decodes its `data` into the documented DTO.
pub(crate) async fn call_data<T: DeserializeOwned>(
    client: &DaemonClient,
    command: &str,
    args: Value,
) -> Result<T, CommandError> {
    call(client, command, args).await?.into_data()
}
