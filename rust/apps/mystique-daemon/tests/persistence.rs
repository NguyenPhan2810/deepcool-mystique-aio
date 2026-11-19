//! Settings persistence: what a successful change writes, what a failure does not, and
//! what the next daemon start reads back into `device_state` and the device init.
//!
//! No hardware and no per-user directory: every test owns a fresh directory under the
//! temp path and names the file explicitly.

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use mystique_daemon::core::Core;
use mystique_daemon::settings_file;
use mystique_ipc::{cmd, CommandOutcome, DeviceState};
use mystique_sensors::SensorSample;

use support::{
    build_core, fast_config, request, FakeEnvironment, FakeProvider, FakeSampler, ProviderState,
};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mystique-persistence-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn core_with_store(state: &Arc<ProviderState>, path: &Path, now: Instant) -> Core {
    build_core(state.clone(), SensorSample::default(), fast_config(), now)
        .with_settings_path(path.to_path_buf())
}

fn send(core: &mut Core, id: u64, command: &str, args: serde_json::Value, now: Instant) {
    let (response, _) = core.handle(&request(id, command, args), now);
    assert!(response.ok, "{command} failed: {response:?}");
    let outcome: CommandOutcome = response.into_data().unwrap();
    assert_eq!(outcome.settings, core.settings(), "{command}");
}

/// The eleven persisted commands, with values that differ from every default.
fn change_every_setting(core: &mut Core, now: Instant) {
    let changes = [
        (1, cmd::SET_ROTATION, serde_json::json!({ "rotation": 2 })),
        (2, cmd::SET_GYRO, serde_json::json!({ "enabled": true })),
        (
            3,
            cmd::SET_BRIGHTNESS,
            serde_json::json!({ "brightness": 33 }),
        ),
        (
            4,
            cmd::SET_IDLE_SCREEN,
            serde_json::json!({ "enabled": false }),
        ),
        (
            5,
            cmd::SET_RGB_SYNC,
            serde_json::json!({ "enabled": false }),
        ),
        (
            6,
            cmd::SET_RECORDING,
            serde_json::json!({ "cpu_clock": false, "cpu_temp": true }),
        ),
        (
            7,
            cmd::SET_AREAS,
            serde_json::json!({ "main": 5, "style": 2, "aux": 1 }),
        ),
        (
            8,
            cmd::SET_MEDIA_TIMING,
            serde_json::json!({ "time": 2, "animation": 1 }),
        ),
        (
            9,
            cmd::SET_LABELS,
            serde_json::json!({
                "cpu": "Intel Core i9-14900K",
                "gpu": "NVIDIA GeForce RTX 4070"
            }),
        ),
        (
            10,
            cmd::SET_FAN_IO,
            serde_json::json!({ "cpu": "pump", "pump": "none" }),
        ),
    ];
    for (id, command, args) in changes {
        send(core, id, command, args, now);
    }
    // Off then on: the final brightness is the lit one, and the restore value follows.
    send(
        core,
        11,
        cmd::SET_SCREEN,
        serde_json::json!({ "enabled": false }),
        now,
    );
    send(
        core,
        12,
        cmd::SET_SCREEN,
        serde_json::json!({ "enabled": true }),
        now,
    );
}

#[test]
fn every_successful_setting_change_reaches_the_file() {
    let dir = temp_dir("all");
    let path = dir.join(settings_file::FILE_NAME);
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = core_with_store(&state, &path, now);
    core.tick(now);
    assert!(core.is_open());

    change_every_setting(&mut core, now);

    assert!(path.is_file(), "the file was written");
    let saved = settings_file::load(&path);
    assert_eq!(saved.snapshot(), core.settings());
    let snapshot = saved.snapshot();
    assert_eq!(snapshot.rotation, 2);
    assert!(snapshot.gyro);
    assert_eq!(snapshot.brightness, 33);
    assert!(snapshot.screen_on);
    assert_eq!(snapshot.screen_restore_brightness, 33);
    assert!(!snapshot.idle_screen);
    assert!(!snapshot.rgb_sync);
    assert!(!snapshot.record_cpu_clock);
    assert!(snapshot.record_cpu_temp);
    assert_eq!(snapshot.main_area, 5);
    assert_eq!(snapshot.style_area, 2);
    assert_eq!(snapshot.aux_area, 1);
    assert_eq!(snapshot.media_seconds, 7);
    assert_eq!(snapshot.media_animation_name, "Scroll");
    assert_eq!(snapshot.cpu_label, "Intel Core i9-14900K");
    assert_eq!(snapshot.gpu_label, "NVIDIA GeForce RTX 4070");
    assert_eq!(snapshot.fan_io_cpu, "pump");
    assert_eq!(snapshot.fan_io_pump, "none");

    // The temp file never survives a completed save.
    assert!(!dir
        .join(format!("{}.tmp", settings_file::FILE_NAME))
        .exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_change_leaves_the_last_good_file_untouched() {
    let dir = temp_dir("failed");
    let path = dir.join(settings_file::FILE_NAME);
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = core_with_store(&state, &path, now);
    core.tick(now);

    send(
        &mut core,
        1,
        cmd::SET_BRIGHTNESS,
        serde_json::json!({ "brightness": 44 }),
        now,
    );
    let before = fs::read(&path).unwrap();
    assert!(String::from_utf8_lossy(&before).contains("\"brightness\": 44"));

    let (response, _) = core.handle(
        &request(
            2,
            cmd::SET_BRIGHTNESS,
            serde_json::json!({ "brightness": 200 }),
        ),
        now,
    );
    assert!(!response.ok);
    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "a range error is not a settings change"
    );

    // A rejected device write is also not persisted.
    let rejected = Arc::new(ProviderState::new());
    let other = dir.join("rejected.json");
    let mut core = core_with_store(&rejected, &other, now);
    core.tick(now);
    rejected
        .rejected_commands
        .store(1, std::sync::atomic::Ordering::SeqCst);
    let (response, _) = core.handle(
        &request(3, cmd::SET_ROTATION, serde_json::json!({ "rotation": 1 })),
        now,
    );
    assert!(!response.ok);
    assert!(!other.exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_first_device_state_and_the_init_carry_the_reloaded_file() {
    let dir = temp_dir("reload");
    let path = dir.join(settings_file::FILE_NAME);
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = core_with_store(&state, &path, now);
    core.tick(now);
    change_every_setting(&mut core, now);
    drop(core);

    // A new process: read the file, then build the core from it. Nothing else carries
    // over between the two.
    let settings = settings_file::load(&path);
    let reopened = Arc::new(ProviderState::new());
    let started = Instant::now();
    let mut core = Core::new(
        Box::new(FakeProvider::new(reopened.clone())),
        Box::new(FakeSampler {
            sample: SensorSample::default(),
        }),
        Box::new(FakeEnvironment::default()),
        fast_config(),
        settings,
        started,
    );

    // `device_state` before the first tick, i.e. before any device traffic.
    let (response, _) = core.handle(
        &request(1, cmd::DEVICE_STATE, serde_json::json!({})),
        started,
    );
    let reported: DeviceState = response.into_data().unwrap();
    assert_eq!(reported.settings.brightness, 33);
    assert_eq!(reported.settings.rotation, 2);
    assert!(reported.settings.gyro);
    assert!(!reported.settings.idle_screen);
    assert!(!reported.settings.rgb_sync);
    assert!(!reported.settings.record_cpu_clock);
    assert_eq!(reported.settings.main_area, 5);
    assert_eq!(reported.settings.media_seconds, 7);
    assert_eq!(reported.settings.cpu_label, "Intel Core i9-14900K");
    assert_eq!(reported.settings.fan_io_cpu, "pump");
    assert_eq!(reported.rotation, Some(2));

    core.tick(started);
    assert!(core.is_open());
    {
        let displays = reopened.init_displays.lock().unwrap();
        let display = displays.last().copied().expect("an init ran");
        assert_eq!(display.brightness(), 33);
        assert_eq!(display.rotation().raw(), 2);
        assert!(display.gyro_allowed());
        assert!(!display.idle_screen());
        assert!(!display.rgb_sync());
    }

    {
        let recordings = reopened.init_recordings.lock().unwrap();
        let recording = recordings.last().copied().expect("an init ran");
        assert!(!recording.cpu_clock());
        assert!(recording.cpu_temp());
    }

    {
        let labels = reopened.init_labels.lock().unwrap();
        let labels = labels.last().expect("an init ran");
        assert_eq!(labels.cpu, "Intel Core i9-14900K");
        assert_eq!(labels.gpu, "NVIDIA GeForce RTX 4070");
    }

    {
        let areas = reopened.init_areas.lock().unwrap();
        let areas = areas.last().expect("an init ran");
        assert_eq!((areas.main(), areas.style(), areas.aux()), (5, 2, 1));
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_daemon_without_a_store_behaves_as_before() {
    // The library's default is no path: tests, the CLI's other entry points and a host
    // with no per-user directory must not write anything.
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = build_core(state, SensorSample::default(), fast_config(), now);
    core.tick(now);
    send(
        &mut core,
        1,
        cmd::SET_BRIGHTNESS,
        serde_json::json!({ "brightness": 21 }),
        now,
    );
    assert_eq!(core.settings().brightness, 21);
}
