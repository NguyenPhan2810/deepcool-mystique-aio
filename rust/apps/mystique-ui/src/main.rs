//! MYSTIQUE LCD control panel.
//!
//! A Tauri 2 desktop shell around the `mystique-daemon` control socket. The daemon owns
//! the USB device and the telemetry loop; this process owns a window. Closing the window
//! does not stop the stream, and no command here opens the panel.
//!
//! The frontend is plain static HTML/CSS/JS embedded from `ui/`; there is no bundler, no
//! npm and no dev server. `cargo build -p mystique-ui` is the whole build.

#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod commands;
mod daemon;
mod error;
mod events;
mod service;

use app::AppState;
use daemon::{Launcher, SpawnRecord};
use tauri::Manager;

fn main() {
    // The client starts before the window: the supervisor connects on its own thread, and
    // notices queue until the event bridge takes them in `setup`.
    let parts = match AppState::connect() {
        Ok(parts) => parts,
        Err(message) => {
            eprintln!("mystique-ui could not start: {message}");
            std::process::exit(1);
        }
    };

    let state = AppState::new(
        parts.client,
        Launcher::sibling(),
        SpawnRecord::default(),
        parts.transport,
        parts.notices,
    );
    let boot = state.clone();

    let built = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .setup(move |app| {
            let state = app.state::<AppState>();
            let Some(notices) = state.take_notices() else {
                return Err(
                    std::io::Error::other("the daemon notice channel was already taken").into(),
                );
            };
            events::spawn(
                app.handle().clone(),
                state.client.clone(),
                state.spawn.clone(),
                notices,
            );

            // Starting the daemon is a blocking probe, so it never runs on the UI thread
            // or inside this hook. The Connection section shows "Contacting the daemon"
            // until the first notice arrives.
            let boot = boot.clone();
            tauri::async_runtime::spawn(async move {
                boot.boot().await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::connection::status,
            commands::connection::device_state,
            commands::connection::open_device,
            commands::connection::close_device,
            commands::connection::stop_service,
            commands::connection::start_service,
            commands::connection::daemon_info,
            commands::connection::daemon_retry,
            commands::connection::daemon_stop,
            commands::settings::set_mode,
            commands::settings::set_fan_io,
            commands::settings::set_brightness,
            commands::settings::set_rotation,
            commands::settings::set_gyro,
            commands::settings::set_idle_screen,
            commands::settings::set_rgb_sync,
            commands::settings::set_recording,
            commands::settings::set_screen_power,
            commands::erase::erase_images,
            commands::raw::raw_packet,
            commands::settings::set_areas,
            commands::settings::set_media_timing,
            commands::settings::set_labels,
            commands::telemetry::start_telemetry,
            commands::telemetry::stop_telemetry,
            commands::telemetry::set_stream_rate,
            commands::telemetry::telemetry_status,
            commands::upload::plan_upload,
            commands::upload::upload_image,
            commands::upload::upload_gif,
        ])
        .build(tauri::generate_context!());

    let app = match built {
        Ok(app) => app,
        Err(error) => {
            eprintln!("mystique-ui could not start: {error}");
            std::process::exit(1);
        }
    };

    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            // The client thread ends and this window drops its child handle. The daemon
            // is never terminated here: it keeps the panel and the stream for the next
            // window, or until someone asks it to stop.
            handle.state::<AppState>().shutdown();
        }
    });
}
