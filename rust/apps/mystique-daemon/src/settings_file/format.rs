//! The on-disk shape and its conversion to and from [`Settings`].
//!
//! Kept apart from the file IO so the JSON contract can be read in one place. Display
//! fields are flat because they are the ones the owner edits most; the rest stay
//! grouped where they share a command byte.

use serde::{Deserialize, Serialize};

use mystique_protocol::{
    DisplayAreas, DisplayConfig, MachineLabels, MediaTiming, RecordingConfig, Rotation,
    BRIGHTNESS_MAX,
};

use super::VERSION;
use crate::state::{parse_rgb_mode, validate_label, FanIo, FanSource, Settings};

/// The file exactly as it is written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct File {
    pub(super) version: u32,
    pub(super) brightness: u8,
    pub(super) rotation: u8,
    pub(super) gyro: bool,
    pub(super) idle_screen: bool,
    pub(super) rgb_sync: bool,
    pub(super) screen_restore_brightness: u8,
    pub(super) recording: RecordingOptions,
    pub(super) areas: AreaSelectors,
    pub(super) media_timing: MediaOptions,
    pub(super) labels: Labels,
    pub(super) fan_io: FanRouting,
    /// `motherboard`, `temperature` or `edge`. Added after version 1 shipped; an older
    /// file without it loads as `motherboard`.
    #[serde(default = "default_rgb_mode")]
    pub(super) rgb_mode: String,
}

fn default_rgb_mode() -> String {
    "motherboard".to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct RecordingOptions {
    pub(super) cpu_clock: bool,
    pub(super) cpu_temp: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct AreaSelectors {
    pub(super) main: u8,
    pub(super) style: u8,
    pub(super) aux: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MediaOptions {
    pub(super) time: u8,
    pub(super) animation: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Labels {
    pub(super) cpu: String,
    pub(super) gpu: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FanRouting {
    pub(super) cpu: String,
    pub(super) pump: String,
}

impl From<&Settings> for File {
    fn from(settings: &Settings) -> Self {
        let display = settings.display;
        Self {
            version: VERSION,
            brightness: display.brightness(),
            rotation: display.rotation().raw(),
            gyro: display.gyro_allowed(),
            idle_screen: display.idle_screen(),
            rgb_sync: display.rgb_sync(),
            screen_restore_brightness: settings.screen_restore_brightness(),
            recording: RecordingOptions {
                cpu_clock: settings.recording.cpu_clock(),
                cpu_temp: settings.recording.cpu_temp(),
            },
            areas: AreaSelectors {
                main: settings.areas.main(),
                style: settings.areas.style(),
                aux: settings.areas.aux(),
            },
            media_timing: MediaOptions {
                time: settings.media_time,
                animation: settings.media_animation,
            },
            labels: Labels {
                cpu: settings.labels.cpu.clone(),
                gpu: settings.labels.gpu.clone(),
            },
            fan_io: FanRouting {
                cpu: settings.fan_io.cpu.as_str().to_owned(),
                pump: settings.fan_io.pump.as_str().to_owned(),
            },
            rgb_mode: settings
                .rgb_mode
                .clone()
                .unwrap_or_else(|| "motherboard".to_owned()),
        }
    }
}

impl TryFrom<File> for Settings {
    type Error = String;

    fn try_from(file: File) -> Result<Self, String> {
        if file.version != VERSION {
            return Err(format!(
                "version {} is not the supported {VERSION}",
                file.version
            ));
        }
        if file.screen_restore_brightness > BRIGHTNESS_MAX {
            return Err(format!(
                "screen-restore brightness {} is outside 0..={BRIGHTNESS_MAX}",
                file.screen_restore_brightness
            ));
        }
        let rotation = Rotation::new(file.rotation).map_err(|error| error.to_string())?;
        let display = DisplayConfig::new(
            file.idle_screen,
            file.gyro,
            rotation,
            file.rgb_sync,
            file.brightness,
        )
        .map_err(|error| error.to_string())?;
        let areas = DisplayAreas::new(file.areas.main, file.areas.style, file.areas.aux)
            .map_err(|error| error.to_string())?;
        MediaTiming::new(file.media_timing.time, file.media_timing.animation)
            .map_err(|error| error.to_string())?;
        validate_label(&file.labels.cpu, "CPU")?;
        validate_label(&file.labels.gpu, "GPU")?;
        let cpu = FanSource::parse(&file.fan_io.cpu).ok_or_else(|| {
            format!(
                "fan source \"{}\" is not cpu, pump or none",
                file.fan_io.cpu
            )
        })?;
        let pump = FanSource::parse(&file.fan_io.pump).ok_or_else(|| {
            format!(
                "fan source \"{}\" is not cpu, pump or none",
                file.fan_io.pump
            )
        })?;
        Ok(Self {
            display,
            recording: RecordingConfig::new(file.recording.cpu_clock, file.recording.cpu_temp),
            screen_restore: Some(file.screen_restore_brightness),
            areas,
            media_time: file.media_timing.time,
            media_animation: file.media_timing.animation,
            labels: MachineLabels::new(file.labels.cpu, file.labels.gpu),
            fan_io: FanIo { cpu, pump },
            rgb_mode: Some(
                parse_rgb_mode(&file.rgb_mode)
                    .unwrap_or("motherboard")
                    .to_owned(),
            ),
            last_rotation: Some(rotation.raw()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> serde_json::Value {
        serde_json::to_value(File::from(&Settings::default())).unwrap()
    }

    #[test]
    fn the_shape_is_the_documented_one() {
        let json = defaults();
        assert_eq!(json["version"], 1);
        assert_eq!(json["brightness"], 70);
        assert_eq!(json["rotation"], 0);
        assert_eq!(json["gyro"], false);
        assert_eq!(json["idle_screen"], true);
        assert_eq!(json["rgb_sync"], true);
        assert_eq!(json["screen_restore_brightness"], 70);
        assert_eq!(
            json["recording"],
            serde_json::json!({ "cpu_clock": true, "cpu_temp": true })
        );
        assert_eq!(
            json["areas"],
            serde_json::json!({ "main": 0, "style": 0, "aux": 0 })
        );
        assert_eq!(
            json["media_timing"],
            serde_json::json!({ "time": 0, "animation": 0 })
        );
        assert_eq!(
            json["labels"],
            serde_json::json!({ "cpu": "--", "gpu": "--" })
        );
        assert_eq!(
            json["fan_io"],
            serde_json::json!({ "cpu": "cpu", "pump": "pump" })
        );
        assert!(
            !json.as_object().unwrap().contains_key("stream_rate"),
            "the stream rate is session-only"
        );
    }

    #[test]
    fn every_field_survives_the_round_trip() {
        let settings = Settings {
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
            rgb_mode: Some("edge".to_owned()),
            last_rotation: Some(3),
        };
        let decoded = Settings::try_from(File::from(&settings)).unwrap();
        assert_eq!(decoded.snapshot(), settings.snapshot());
        assert_eq!(decoded.last_rotation, Some(3));
    }

    #[test]
    fn nonsense_is_an_error_not_a_silent_clamp() {
        let mut file = File::from(&Settings::default());
        file.brightness = 200;
        assert!(Settings::try_from(file.clone()).is_err());

        file.brightness = 40;
        file.rotation = 4;
        assert!(Settings::try_from(file.clone()).is_err());

        file.rotation = 0;
        file.labels.cpu = String::new();
        assert!(Settings::try_from(file.clone()).is_err());

        file.labels.cpu = "--".to_owned();
        file.fan_io.cpu = "gpu".to_owned();
        assert!(Settings::try_from(file).is_err());

        let mut version = File::from(&Settings::default());
        version.version = VERSION + 1;
        assert!(Settings::try_from(version).is_err());
    }
}
