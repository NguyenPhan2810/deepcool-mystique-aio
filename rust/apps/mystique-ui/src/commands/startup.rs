//! Startup toggles. The OS entry is the source of truth, so a change made outside the app
//! is reflected the next time the sheet opens.

use crate::startup::{self, StartupState};

#[tauri::command]
pub async fn startup_get() -> Result<StartupState, String> {
    Ok(startup::read())
}

/// Writes the OS entry and returns what the OS now reports, not what was requested.
#[tauri::command]
pub async fn startup_set(enabled: bool, minimized: bool) -> Result<StartupState, String> {
    startup::write(enabled, minimized)?;
    Ok(startup::read())
}
