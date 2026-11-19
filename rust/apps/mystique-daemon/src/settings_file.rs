//! The daemon's settings file.
//!
//! Everything the owner changes used to last one session: restarting the daemon reset
//! brightness, rotation, gyro, idle screen, RGB sync, recording options, display areas,
//! media timing, labels and fan routing. This module keeps them in one versioned JSON
//! object in the per-user directory.
//!
//! # Rules
//!
//! - The file is read **before** the device init, so the first `0x02`/`0x04`/`0x05`
//!   payload and the two labels already carry the saved state.
//! - It is written after every successful settings command, atomically: the new body
//!   goes to `settings.json.tmp` and is renamed over the old file. A failed rename
//!   leaves the previous file intact.
//! - A write failure is reported on stderr and never fails the command. Persistence is
//!   a convenience, not a precondition for driving the panel.
//! - A missing file is normal and silent. A file that cannot be read, parsed or applied
//!   is reported once and ignored, and the daemon starts on its defaults.
//! - The telemetry stream rate is **not** here. It is a host property the CLI owns, and
//!   it is deliberately session-only.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use mystique_ipc::cmd;
use mystique_protocol::Rotation;

use crate::state::Settings;

mod format;

use format::File;

/// The on-disk format this build writes and understands.
pub const VERSION: u32 = 1;
/// The file name inside the per-user directory.
pub const FILE_NAME: &str = "settings.json";

/// `%LOCALAPPDATA%\mystique\settings.json`, or the temp directory when that is unset.
#[cfg(windows)]
pub fn default_settings_path() -> Result<PathBuf, String> {
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        if !base.is_empty() {
            return Ok(PathBuf::from(base).join("mystique").join(FILE_NAME));
        }
    }
    Ok(std::env::temp_dir().join("mystique-daemon").join(FILE_NAME))
}

/// `$XDG_CONFIG_HOME/mystique/settings.json`, or `~/.config/mystique/settings.json`.
///
/// Configuration, not runtime state: this survives a reboot, unlike the socket in
/// `$XDG_RUNTIME_DIR`.
#[cfg(unix)]
pub fn default_settings_path() -> Result<PathBuf, String> {
    if let Some(base) = std::env::var_os("XDG_CONFIG_HOME") {
        if !base.is_empty() {
            return Ok(PathBuf::from(base).join("mystique").join(FILE_NAME));
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        if !home.is_empty() {
            return Ok(PathBuf::from(home)
                .join(".config")
                .join("mystique")
                .join(FILE_NAME));
        }
    }
    Err("cannot place the settings file: XDG_CONFIG_HOME and HOME are unset".to_owned())
}

/// Reads the file. Every failure path yields the defaults and at most one stderr line;
/// a missing file is the first-run case and says nothing.
pub fn load(path: &Path) -> Settings {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Settings::default(),
        Err(error) => {
            eprintln!(
                "mystique-daemon: could not read the settings file {}: {error}; using defaults",
                path.display()
            );
            return Settings::default();
        }
    };
    let file: File = match serde_json::from_str(&text) {
        Ok(file) => file,
        Err(error) => {
            eprintln!(
                "mystique-daemon: the settings file {} is not valid JSON for version {VERSION} \
                 ({error}); using defaults",
                path.display()
            );
            return Settings::default();
        }
    };
    match Settings::try_from(file) {
        Ok(settings) => settings,
        Err(message) => {
            eprintln!(
                "mystique-daemon: the settings file {} was ignored: {message}; using defaults",
                path.display()
            );
            Settings::default()
        }
    }
}

/// Writes `settings` atomically. The error is the stderr sentence the caller prints;
/// it never reaches the device path.
pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    let body = serde_json::to_string_pretty(&File::from(settings))
        .map_err(|error| format!("could not encode the settings: {error}"))?;
    let temp = temp_path(path);
    let written = write_body(&temp, &body);
    if let Err(message) = written {
        let _ = fs::remove_file(&temp);
        return Err(message);
    }
    fs::rename(&temp, path).map_err(|error| {
        let _ = fs::remove_file(&temp);
        format!("could not replace {}: {error}", path.display())
    })
}

fn write_body(path: &Path, body: &str) -> Result<(), String> {
    let mut file = fs::File::create(path)
        .map_err(|error| format!("could not create {}: {error}", path.display()))?;
    file.write_all(body.as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("could not write {}: {error}", path.display()))
}

fn temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| FILE_NAME.to_owned());
    let mut temp = path.to_path_buf();
    temp.set_file_name(format!("{name}.tmp"));
    temp
}

/// True when a successful answer to this command changes the file. The stream rate is
/// absent on purpose; `set_mode` is a device mode, not a persisted preference.
pub fn persists(command: &str) -> bool {
    const PERSISTED: &[&str] = &[
        cmd::SET_BRIGHTNESS,
        cmd::SET_ROTATION,
        cmd::SET_GYRO,
        cmd::SET_SCREEN,
        cmd::SET_IDLE_SCREEN,
        cmd::SET_RGB_SYNC,
        cmd::SET_RGB_MODE,
        cmd::SET_RECORDING,
        cmd::SET_AREAS,
        cmd::SET_MEDIA_TIMING,
        cmd::SET_LABELS,
        cmd::SET_FAN_IO,
    ];
    PERSISTED.contains(&command)
}

/// Explicit command-line values. `None` means the owner did not type the flag, so the
/// persisted value (or the default) stands. This is why the flags have no defaults.
#[derive(Debug, Clone, Default)]
pub struct StartupOverrides {
    pub rotation: Option<Rotation>,
    pub cpu: Option<String>,
    pub gpu: Option<String>,
}

/// Applies the flags that were actually given on top of the loaded file.
pub fn apply_startup(settings: &mut Settings, overrides: StartupOverrides) {
    if let Some(rotation) = overrides.rotation {
        settings.display = settings.display.with_rotation(rotation);
        settings.last_rotation = Some(rotation.raw());
    }
    if let Some(cpu) = overrides.cpu {
        settings.labels.cpu = cpu;
    }
    if let Some(gpu) = overrides.gpu {
        settings.labels.gpu = gpu;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{FanIo, FanSource};
    use mystique_protocol::{DisplayAreas, DisplayConfig, MachineLabels, RecordingConfig};

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mystique-settings-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn rich_settings() -> Settings {
        Settings {
            display: DisplayConfig::new(false, true, Rotation::new(3).unwrap(), false, 31).unwrap(),
            screen_restore: Some(88),
            recording: RecordingConfig::new(false, true),
            areas: DisplayAreas::new(5, 2, 1).unwrap(),
            media_time: 2,
            media_animation: 1,
            labels: MachineLabels::new("Intel Core i9-14900K", "NVIDIA GeForce RTX 4070"),
            fan_io: FanIo {
                cpu: FanSource::Pump,
                pump: FanSource::None,
            },
            rgb_mode: Some("temperature".to_owned()),
            last_rotation: Some(3),
        }
    }

    #[test]
    fn every_setting_round_trips_through_the_file() {
        let dir = temp_dir("round-trip");
        let path = dir.join(FILE_NAME);
        let settings = rich_settings();
        save(&path, &settings).unwrap();

        let loaded = load(&path);
        let expected = settings.snapshot();
        let actual = loaded.snapshot();
        assert_eq!(actual, expected);
        assert_eq!(loaded.last_rotation, Some(3));
        assert!(!dir.join(format!("{FILE_NAME}.tmp")).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_is_the_first_run_and_keeps_the_defaults() {
        let dir = temp_dir("missing");
        let loaded = load(&dir.join(FILE_NAME));
        assert_eq!(loaded.snapshot(), Settings::default().snapshot());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_or_unknown_file_keeps_the_defaults() {
        let dir = temp_dir("corrupt");
        let path = dir.join(FILE_NAME);

        fs::write(&path, "{ not json").unwrap();
        assert_eq!(load(&path).snapshot(), Settings::default().snapshot());

        fs::write(
            &path,
            serde_json::json!({
                "version": 2,
                "brightness": 10, "rotation": 0, "gyro": false, "idle_screen": true,
                "rgb_sync": true, "screen_restore_brightness": 10,
                "recording": { "cpu_clock": true, "cpu_temp": true },
                "areas": { "main": 0, "style": 0, "aux": 0 },
                "media_timing": { "time": 0, "animation": 0 },
                "labels": { "cpu": "--", "gpu": "--" },
                "fan_io": { "cpu": "cpu", "pump": "pump" }
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(load(&path).snapshot(), Settings::default().snapshot());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_out_of_range_value_is_treated_as_corrupt_not_clamped() {
        let dir = temp_dir("range");
        let path = dir.join(FILE_NAME);
        let mut json = serde_json::to_value(File::from(&Settings::default())).unwrap();
        json["brightness"] = serde_json::json!(200);
        fs::write(&path, json.to_string()).unwrap();
        assert_eq!(load(&path).snapshot(), Settings::default().snapshot());

        json["brightness"] = serde_json::json!(40);
        json["fan_io"] = serde_json::json!({ "cpu": "gpu", "pump": "pump" });
        fs::write(&path, json.to_string()).unwrap();
        assert_eq!(load(&path).snapshot(), Settings::default().snapshot());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_creates_the_directory_and_leaves_no_temp_file() {
        let dir = temp_dir("create");
        let path = dir.join("nested").join(FILE_NAME);
        save(&path, &Settings::default()).unwrap();
        assert!(path.is_file());
        assert!(!path.with_file_name(format!("{FILE_NAME}.tmp")).exists());
        // A second save replaces the first file; no append and no stale body.
        save(&path, &rich_settings()).unwrap();
        assert_eq!(load(&path).snapshot().brightness, 31);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_setting_changes_are_persisted() {
        for command in [
            cmd::SET_BRIGHTNESS,
            cmd::SET_ROTATION,
            cmd::SET_GYRO,
            cmd::SET_SCREEN,
            cmd::SET_IDLE_SCREEN,
            cmd::SET_RGB_SYNC,
            cmd::SET_RGB_MODE,
            cmd::SET_RECORDING,
            cmd::SET_AREAS,
            cmd::SET_MEDIA_TIMING,
            cmd::SET_LABELS,
            cmd::SET_FAN_IO,
        ] {
            assert!(persists(command), "{command}");
        }
        for command in [
            cmd::SET_STREAM_RATE,
            cmd::SET_MODE,
            cmd::STATUS,
            cmd::DEVICE_STATE,
            cmd::ERASE_IMAGES,
        ] {
            assert!(!persists(command), "{command}");
        }
    }

    #[test]
    fn the_flags_only_override_what_the_owner_typed() {
        let mut settings = rich_settings();
        apply_startup(&mut settings, StartupOverrides::default());
        assert_eq!(settings.snapshot(), rich_settings().snapshot());

        apply_startup(
            &mut settings,
            StartupOverrides {
                rotation: Some(Rotation::new(1).unwrap()),
                cpu: Some("CPU FROM FLAG".to_owned()),
                gpu: None,
            },
        );
        let snapshot = settings.snapshot();
        assert_eq!(snapshot.rotation, 1);
        assert_eq!(snapshot.cpu_label, "CPU FROM FLAG");
        assert_eq!(
            snapshot.gpu_label, "NVIDIA GeForce RTX 4070",
            "an absent flag keeps the file's label"
        );
        assert_eq!(settings.last_rotation, Some(1));
    }
}
