//! The headless MYSTIQUE daemon.
//!
//! It owns the panel for the process lifetime, runs the machine-info init, streams
//! `0x01` telemetry at 1 Hz, and serves the Tauri-shaped control API over a local
//! socket. The desktop UI and the CLI are not re-pointed at it yet; see `README.md`.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::Parser;
use mystique_daemon::core::{Core, CoreConfig};
use mystique_daemon::endpoint::Endpoint;
use mystique_daemon::lock::InstanceLock;
use mystique_daemon::state::Settings;
use mystique_daemon::{cli, environment, lock, panel, server, settings_file, worker};
use mystique_ipc::{ServerMessage, PROTOCOL_VERSION};
use tokio::sync::{broadcast, watch};

/// Broadcast history per client. At 1 Hz this is minutes of backlog, and a client that
/// falls further behind is disconnected rather than served stale frames.
const EVENT_BUFFER: usize = 256;

fn main() -> ExitCode {
    let options = cli::Options::parse();

    if options.service {
        eprintln!(
            "mystique-daemon: --service is not implemented in this build. The binary is a \
             console application and does not call StartServiceCtrlDispatcher.\n\
             Run it from Task Scheduler or a service wrapper; see \
             rust/packaging/windows/README.md."
        );
        return ExitCode::from(2);
    }

    let overrides = match options.startup_overrides() {
        Ok(overrides) => overrides,
        Err(message) => {
            eprintln!("mystique-daemon: {message}");
            return ExitCode::from(2);
        }
    };

    let endpoint = match Endpoint::from_option(options.endpoint.as_deref()) {
        Ok(endpoint) => endpoint,
        Err(message) => {
            eprintln!("mystique-daemon: {message}");
            return ExitCode::from(2);
        }
    };

    let lock_path = match lock::default_lock_path() {
        Ok(path) => path,
        Err(message) => {
            eprintln!("mystique-daemon: {message}");
            return ExitCode::from(1);
        }
    };
    let instance_lock = match InstanceLock::acquire(&lock_path) {
        Ok(lock) => lock,
        Err(error) => {
            eprintln!("mystique-daemon: {error}");
            return ExitCode::from(1);
        }
    };

    // The lock is held first: a second daemon must fail before it touches the socket or
    // the device. A stale Unix socket left by a crash is then provably stale.
    if let Err(message) = endpoint.prepare() {
        eprintln!("mystique-daemon: {message}");
        return ExitCode::from(1);
    }

    // Before anything can open the panel: the first init and every device_state answer
    // carry the saved configuration, not the crate defaults. A per-user directory that
    // cannot be found costs persistence, not the daemon.
    let settings_path = match settings_file::default_settings_path() {
        Ok(path) => Some(path),
        Err(message) => {
            eprintln!("mystique-daemon: settings will not be saved: {message}");
            None
        }
    };
    let mut settings = match settings_path.as_deref() {
        Some(path) => settings_file::load(path),
        None => Settings::default(),
    };
    settings_file::apply_startup(&mut settings, overrides);
    let config = CoreConfig {
        interval: Duration::from_millis(options.interval_ms),
        ..CoreConfig::default()
    };

    eprintln!(
        "mystique-daemon: protocol v{PROTOCOL_VERSION}; endpoint {}; telemetry every {} ms",
        endpoint.display(),
        options.interval_ms
    );
    if let Some(path) = settings_path.as_deref() {
        eprintln!("mystique-daemon: settings {}", path.display());
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("mystique-daemon: could not start the async runtime: {error}");
            return ExitCode::from(1);
        }
    };

    let result = runtime.block_on(run(
        endpoint.clone(),
        config,
        settings,
        settings_path,
        instance_lock,
    ));
    endpoint.cleanup();

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("mystique-daemon: {message}");
            ExitCode::from(1)
        }
    }
}

async fn run(
    endpoint: Endpoint,
    config: CoreConfig,
    settings: Settings,
    settings_path: Option<PathBuf>,
    _lock: InstanceLock,
) -> Result<(), String> {
    let (events, _) = broadcast::channel::<ServerMessage>(EVENT_BUFFER);

    let mut core = Core::new(
        Box::new(panel::RealProvider),
        mystique_sensors::default_sampler(),
        Box::new(environment::RealEnvironment),
        config,
        settings,
        Instant::now(),
    );
    if let Some(path) = settings_path {
        core = core.with_settings_path(path);
    }
    let worker = worker::Worker::spawn(core, events.clone())
        .map_err(|error| format!("could not start the device worker: {error}"))?;

    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    let mut server = tokio::spawn(server::serve(
        endpoint.clone(),
        worker.sender(),
        events,
        shutdown_tx.clone(),
    ));

    let signal = tokio::select! {
        biased;
        // The `shutdown` command breaks the accept loop by itself, but this branch is
        // polled first: a requested stop is never reported as a server failure.
        _ = shutdown_requested(&mut shutdown_rx) => "a shutdown request",
        signal = wait_for_signal() => signal,
        result = &mut server => {
            return Err(match result {
                Ok(Ok(())) => "the control server stopped unexpectedly".to_owned(),
                Ok(Err(error)) => format!("the control server failed: {error}"),
                Err(error) => format!("the control server task failed: {error}"),
            });
        }
    };

    eprintln!("mystique-daemon: {signal}; stopping the stream and releasing the panel");
    let _ = shutdown_tx.send(true);
    if tokio::time::timeout(Duration::from_secs(1), &mut server)
        .await
        .is_err()
    {
        server.abort();
    }
    if !worker.shutdown().await {
        eprintln!("mystique-daemon: the device worker did not acknowledge the shutdown in time");
    }
    Ok(())
}

/// Resolves when any clone of the shutdown sender flips the value to true. A dropped
/// channel cannot request a stop, so that case simply waits forever; the signal path and
/// the server task still end the process.
async fn shutdown_requested(rx: &mut watch::Receiver<bool>) {
    while rx.changed().await.is_ok() {
        if *rx.borrow_and_update() {
            return;
        }
    }
    std::future::pending::<()>().await
}

/// Ctrl+C everywhere, plus SIGTERM on Unix. A service manager's stop is a signal.
async fn wait_for_signal() -> &'static str {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => tokio::select! {
                _ = tokio::signal::ctrl_c() => "SIGINT",
                _ = terminate.recv() => "SIGTERM",
            },
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
                "SIGINT"
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        "Ctrl+C"
    }
}
