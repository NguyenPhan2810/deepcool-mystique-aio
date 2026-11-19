//! The client request envelope and the typed arguments of every command.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::IpcError;

/// Command names, exactly as the Tauri command names they mirror.
pub mod cmd {
    pub const STATUS: &str = "status";
    pub const DEVICE_STATE: &str = "device_state";
    pub const OPEN_DEVICE: &str = "open_device";
    pub const CLOSE_DEVICE: &str = "close_device";
    pub const SET_BRIGHTNESS: &str = "set_brightness";
    pub const SET_ROTATION: &str = "set_rotation";
    pub const SET_GYRO: &str = "set_gyro";
    pub const SET_SCREEN: &str = "set_screen";
    pub const SET_IDLE_SCREEN: &str = "set_idle_screen";
    pub const SET_RGB_SYNC: &str = "set_rgb_sync";
    pub const SET_RGB_MODE: &str = "set_rgb_mode";
    pub const SET_RECORDING: &str = "set_recording";
    pub const SET_MODE: &str = "set_mode";
    pub const SET_FAN_IO: &str = "set_fan_io";
    pub const SET_AREAS: &str = "set_areas";
    pub const SET_MEDIA_TIMING: &str = "set_media_timing";
    pub const SET_LABELS: &str = "set_labels";
    /// The CPU and GPU names detected on the host, for the label pickers. No arguments.
    pub const DETECTED_LABELS: &str = "detected_labels";
    pub const RAW_PACKET: &str = "raw_packet";
    /// `0x14` erase. The Tauri app's own command name; there is no rename here.
    pub const ERASE_IMAGES: &str = "erase_images";
    pub const PLAN_UPLOAD: &str = "plan_upload";
    pub const UPLOAD_IMAGE: &str = "upload_image";
    pub const UPLOAD_GIF: &str = "upload_gif";
    pub const START_STREAM: &str = "start_stream";
    pub const STOP_STREAM: &str = "stop_stream";
    /// Changes the telemetry period without stopping the stream. `hz` is the rate;
    /// the panel is not the limit, the host timer is.
    pub const SET_STREAM_RATE: &str = "set_stream_rate";
    /// Stops the daemon itself: the reply goes out first, then the stream, the panel and
    /// the control endpoint are released and the process exits.
    pub const SHUTDOWN: &str = "shutdown";

    /// Every command the daemon implements, in the order this document lists them.
    /// `set_screen` replaces the Tauri app's `set_screen_power`; the argument is
    /// `enabled`, and "off" is brightness 0 on the wire.
    pub const ALL: &[&str] = &[
        STATUS,
        DEVICE_STATE,
        OPEN_DEVICE,
        CLOSE_DEVICE,
        SET_BRIGHTNESS,
        SET_ROTATION,
        SET_GYRO,
        SET_SCREEN,
        SET_IDLE_SCREEN,
        SET_RGB_SYNC,
        SET_RGB_MODE,
        SET_RECORDING,
        SET_MODE,
        SET_FAN_IO,
        SET_AREAS,
        SET_MEDIA_TIMING,
        SET_LABELS,
        DETECTED_LABELS,
        RAW_PACKET,
        ERASE_IMAGES,
        PLAN_UPLOAD,
        UPLOAD_IMAGE,
        UPLOAD_GIF,
        START_STREAM,
        STOP_STREAM,
        SET_STREAM_RATE,
        SHUTDOWN,
    ];

    pub fn is_known(name: &str) -> bool {
        ALL.contains(&name)
    }
}

/// One client request. `id` is chosen by the client and echoed in the response; the
/// daemon does not require it to be unique, but a client that multiplexes should use a
/// counter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub cmd: String,
    /// A command's arguments. Always present on the wire; `{}` for commands with none.
    #[serde(default)]
    pub args: Value,
}

impl Request {
    pub fn new(id: u64, cmd: impl Into<String>) -> Self {
        Self {
            id,
            cmd: cmd.into(),
            args: Value::Object(Map::new()),
        }
    }

    pub fn with_args<T: Serialize>(
        id: u64,
        cmd: impl Into<String>,
        args: &T,
    ) -> Result<Self, IpcError> {
        Ok(Self {
            id,
            cmd: cmd.into(),
            args: serde_json::to_value(args)?,
        })
    }

    /// Parses `args` into a command's argument type. An absent (`null`) `args` is
    /// treated as `{}`, so the message is the same either way.
    pub fn parse_args<T: DeserializeOwned>(&self) -> Result<T, IpcError> {
        let value = if self.args.is_null() {
            Value::Object(Map::new())
        } else {
            self.args.clone()
        };
        serde_json::from_value(value).map_err(|error| IpcError::Arguments {
            command: self.cmd.clone(),
            message: error.to_string(),
        })
    }
}

/// Typed argument objects, named after the Tauri parameters they mirror.
pub mod args {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Brightness {
        pub brightness: u8,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Rotation {
        pub rotation: u8,
    }

    /// Used by `set_gyro`, `set_screen`, `set_idle_screen` and `set_rgb_sync`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Enabled {
        pub enabled: bool,
    }

    /// Used by `set_stream_rate`. The UI offers 1/5/10/20/30; the daemon accepts more
    /// because the link does.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub struct StreamRate {
        pub hz: u32,
    }

    /// Used by `set_mode`: `1` Monitor, `2` Media, `3` Recording (`0x03`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Mode {
        pub mode: u8,
    }

    /// Used by `set_fan_io`. Each value is `cpu`, `pump` or `none`; it selects which
    /// sensor feeds that dial. Host routing, not a device command.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct FanIo {
        pub cpu: String,
        pub pump: String,
    }

    /// Used by `set_rgb_mode`: `motherboard`, `temperature` or `edge`. Only one lighting
    /// source can be active; the vendor app interlocks the same three.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct RgbMode {
        pub mode: String,
    }

    /// `0x05`: the two recording graph options.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Recording {
        pub cpu_clock: bool,
        pub cpu_temp: bool,
    }

    /// A raw 48-byte packet request. The daemon builds the packet, so the UI never
    /// computes a checksum.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct RawPacket {
        pub command: u8,
        /// Up to 39 bytes. The protocol crate enforces the ceiling.
        #[serde(default)]
        pub payload: Vec<u8>,
        /// `init` or `data`.
        pub pipe: String,
        /// Build and print the packet, but do not send it. Required, so a client cannot
        /// omit it and send by accident.
        pub dry_run: bool,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Areas {
        pub main: u8,
        /// Style Type at `0x04` index 1. Defaults to 0 (Normal) so a client written
        /// against the old two-selector shape still deserialises.
        #[serde(default)]
        pub style: u8,
        pub aux: u8,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub struct MediaTiming {
        pub time: u8,
        pub animation: u8,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Labels {
        #[serde(default)]
        pub cpu: Option<String>,
        #[serde(default)]
        pub gpu: Option<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Upload {
        pub path: String,
        /// `cover` (default) or `contain`.
        #[serde(default)]
        pub fit: Option<String>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_without_arguments_carries_an_empty_object() {
        let request = Request::new(7, cmd::STATUS);
        assert_eq!(request.args, Value::Object(Map::new()));
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["id"], 7);
        assert_eq!(json["cmd"], "status");
        assert_eq!(json["args"], serde_json::json!({}));
    }

    #[test]
    fn typed_arguments_round_trip_and_an_absent_args_object_is_empty() {
        let request =
            Request::with_args(1, cmd::SET_BRIGHTNESS, &args::Brightness { brightness: 44 })
                .unwrap();
        assert_eq!(
            request.parse_args::<args::Brightness>().unwrap(),
            args::Brightness { brightness: 44 }
        );

        let mut bare = Request::new(2, cmd::SET_BRIGHTNESS);
        bare.args = Value::Null;
        assert!(bare.parse_args::<args::Brightness>().is_err());
    }

    #[test]
    fn every_documented_command_name_is_known() {
        for name in cmd::ALL {
            assert!(cmd::is_known(name), "{name}");
        }
        assert!(cmd::is_known(cmd::SET_IDLE_SCREEN));
        assert!(cmd::is_known(cmd::SET_RGB_SYNC));
        assert!(cmd::is_known(cmd::SET_RECORDING));
        assert!(cmd::is_known(cmd::DETECTED_LABELS));
        assert!(cmd::is_known(cmd::RAW_PACKET));
        assert!(!cmd::is_known("set_screen_power"));
        assert!(!cmd::is_known(""));
    }

    #[test]
    fn the_new_setting_arguments_round_trip() {
        let request = Request::with_args(
            1,
            cmd::SET_RECORDING,
            &args::Recording {
                cpu_clock: false,
                cpu_temp: true,
            },
        )
        .unwrap();
        assert_eq!(
            request.parse_args::<args::Recording>().unwrap(),
            args::Recording {
                cpu_clock: false,
                cpu_temp: true
            }
        );

        let raw = Request::with_args(
            2,
            cmd::RAW_PACKET,
            &args::RawPacket {
                command: 0xA1,
                payload: vec![0x01, 0x02],
                pipe: "init".to_owned(),
                dry_run: true,
            },
        )
        .unwrap();
        let parsed = raw.parse_args::<args::RawPacket>().unwrap();
        assert_eq!(parsed.command, 0xA1);
        assert_eq!(parsed.payload, [0x01, 0x02]);
        assert_eq!(parsed.pipe, "init");
        assert!(parsed.dry_run);
    }

    #[test]
    fn a_bad_argument_shape_reports_the_command() {
        let mut request = Request::new(3, cmd::SET_BRIGHTNESS);
        request.args = serde_json::json!({ "brightness": "bright" });
        let error = request.parse_args::<args::Brightness>().unwrap_err();
        let message = error.to_string();
        assert!(message.contains("set_brightness"), "{message}");
        assert!(message.contains("brightness"), "{message}");
    }
}
