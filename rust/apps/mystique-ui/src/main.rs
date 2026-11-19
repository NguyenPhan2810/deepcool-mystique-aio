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
mod startup;

use app::AppState;
use daemon::{Launcher, SpawnRecord};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WindowEvent};

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
    let start_hidden = std::env::args().any(|argument| argument == "--minimized");

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

            build_tray(app)?;

            // The window is created hidden so a `--minimized` launch never flashes it.
            // A normal launch shows it, exactly as before.
            if !start_hidden {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                }
            }

            // Starting the daemon is a blocking probe, so it never runs on the UI thread
            // or inside this hook. The Connection section shows "Contacting the daemon"
            // until the first notice arrives.
            let boot = boot.clone();
            tauri::async_runtime::spawn(async move {
                boot.boot().await;
            });
            Ok(())
        })
        // The window is a control surface, not the application. Closing it leaves the
        // tray icon and the daemon running; Quit in the tray menu is the exit.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
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
            commands::settings::set_rgb_mode,
            commands::settings::set_recording,
            commands::settings::set_screen_power,
            commands::erase::erase_images,
            commands::raw::raw_packet,
            commands::settings::set_areas,
            commands::settings::set_media_timing,
            commands::settings::set_labels,
            commands::labels::detected_labels,
            commands::telemetry::start_telemetry,
            commands::telemetry::stop_telemetry,
            commands::telemetry::set_stream_rate,
            commands::telemetry::telemetry_status,
            commands::startup::startup_get,
            commands::startup::startup_set,
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

/// Tray icon and menu. The window has no exit path of its own: closing hides it, and only
/// `Quit` ends the process. The daemon is never terminated by closing anything — `Stop
/// daemon` asks it to shut down over the socket, exactly like the button in the session
/// sheet, and the panel goes dark as part of that stop.
fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show control panel", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop_daemon", "Stop daemon", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &stop, &quit])?;

    let mut tray = TrayIconBuilder::with_id("mystique")
        .tooltip("MYSTIQUE Control")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_window(app),
            "stop_daemon" => request_daemon_stop(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn show_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// `shutdown` over the control socket. The daemon answers first, then stops the stream,
/// blanks the panel and releases the device; the UI stays open with no daemon attached.
fn request_daemon_stop(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let _ = commands::call_data::<mystique_ipc::ShutdownAck>(
            &state.client,
            mystique_ipc::cmd::SHUTDOWN,
            serde_json::json!({}),
        )
        .await;
    });
}
