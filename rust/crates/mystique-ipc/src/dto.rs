//! Serialisable DTOs, field for field the ones the Tauri UI documents.
//!
//! Anything added for the daemon is optional (`DeviceState::telemetry`), so a client
//! built against the Tauri shapes deserialises these payloads without changes.

use serde::{Deserialize, Serialize};

use crate::event::TelemetryStatus;

fn default_true() -> bool {
    true
}

fn default_fan_cpu() -> String {
    "cpu".to_owned()
}

fn default_fan_pump() -> String {
    "pump".to_owned()
}

/// The app's current belief about the panel's settings. Values are what was sent, or
/// the protocol crate's defaults before anything was sent — never a device read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsState {
    pub gyro: bool,
    pub rotation: u8,
    pub rotation_degrees: u16,
    pub brightness: u8,
    /// `0x02[0]`: idle screen display. Not screen power.
    #[serde(default = "default_true")]
    pub idle_screen: bool,
    /// `0x02[3]`: RGB light effect sync.
    #[serde(default)]
    pub rgb_sync: bool,
    /// `0x05[0]`: CPU frequency graph in Recording mode.
    #[serde(default = "default_true")]
    pub record_cpu_clock: bool,
    /// `0x05[1]`: CPU temperature graph in Recording mode.
    #[serde(default = "default_true")]
    pub record_cpu_temp: bool,
    /// Derived, never sent: brightness 0 blanks the panel.
    pub screen_on: bool,
    /// What "screen on" restores. 70 until a non-zero brightness is set.
    pub screen_restore_brightness: u8,
    pub main_area: u8,
    pub style_area: u8,
    pub aux_area: u8,
    pub media_time: u8,
    pub media_seconds: u8,
    pub media_animation: u8,
    pub media_animation_name: String,
    pub cpu_label: String,
    pub gpu_label: String,
    /// Which sensor feeds the panel's first RPM dial: `cpu`, `pump` or `none`.
    #[serde(default = "default_fan_cpu")]
    pub fan_io_cpu: String,
    /// Which sensor feeds the panel's second RPM dial.
    #[serde(default = "default_fan_pump")]
    pub fan_io_pump: String,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            gyro: false,
            rotation: 0,
            rotation_degrees: 0,
            brightness: 70,
            idle_screen: true,
            rgb_sync: false,
            record_cpu_clock: true,
            record_cpu_temp: true,
            screen_on: true,
            screen_restore_brightness: 70,
            main_area: 0,
            style_area: 0,
            aux_area: 0,
            media_time: 0,
            media_seconds: 3,
            media_animation: 0,
            media_animation_name: "Split Display".to_owned(),
            cpu_label: "--".to_owned(),
            gpu_label: "--".to_owned(),
            fan_io_cpu: default_fan_cpu(),
            fan_io_pump: default_fan_pump(),
        }
    }
}

/// Connection and session state. Identical to the Tauri `DeviceState`; `telemetry` is
/// the one additive field, and it is absent when a Tauri-shaped client omits it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceState {
    pub device_open: bool,
    pub device_present: bool,
    pub service_running: bool,
    pub service_supported: bool,
    /// Verbatim `sc.exe query MystiqueService` text on Windows, or the reason the
    /// query does not apply on this platform.
    pub service_output: String,
    pub last_error: Option<String>,
    /// Last rotation this session set, or the last `0x10` read.
    pub rotation: Option<u8>,
    pub settings: SettingsState,
    /// Daemon extension: the telemetry stream's state. Absent in the Tauri DTO.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<TelemetryStatus>,
}

/// `DeviceState` plus a live `0x10` reading. Flattened exactly like the Tauri DTO, so
/// the JSON is one object, not a nested `device`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusDto {
    #[serde(flatten)]
    pub device: DeviceState,
    /// `0x10 reply (advisory)`, `session (last set)` or `unknown`.
    pub rotation_source: String,
    pub rotation_degrees: Option<u16>,
    pub mode: Option<String>,
    pub mode_raw: Option<u8>,
    pub result: Option<String>,
    /// The exact `0x10` request when this call performed a live read.
    pub packet_hex: Option<String>,
}

/// Descriptor summary of the opened panel. No serial, no device identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub product: Option<String>,
    pub bus: u8,
    pub address: u8,
    pub release: String,
    pub endpoints: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenResult {
    pub already_open: bool,
    pub message: String,
    pub device: Option<DeviceInfo>,
    /// False when the four documented endpoints are not all present.
    pub endpoints_ok: bool,
}

/// The answer to `shutdown`, written before anything stops. `stopping` is the daemon's
/// acknowledgement that it accepted the request; the stream and the panel are released
/// after this line reaches the client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShutdownAck {
    pub stopping: bool,
    pub message: String,
}

/// Success shape for every settings command and both uploads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandOutcome {
    pub packets: Vec<crate::response::PacketRecord>,
    pub settings: SettingsState,
    pub message: String,
}

/// The raw packet panel's result. Dry runs build the packet without touching USB;
/// sends add the exact record that the UI appends to its packet log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawPacketOutcome {
    pub command: u8,
    pub pipe: String,
    pub payload_hex: String,
    /// The exact 48-byte request, checksum and trailer included.
    pub packet_hex: String,
    pub dry_run: bool,
    pub sent: bool,
    /// The device's verdict when a send happened.
    pub result: Option<String>,
    pub result_code: Option<u8>,
    pub reply_hex: Option<String>,
    /// The record to append to the packet log. `None` for a dry run.
    pub record: Option<crate::response::PacketRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UploadKind {
    Image,
    Gif,
}

impl UploadKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Gif => "gif",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanCommandSummary {
    pub label: String,
    pub pipe: String,
    pub payload_hex: String,
    pub packet_hex: String,
}

/// What the UI shows before the upload confirm step. Counts come from the real plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadPlanSummary {
    pub path: String,
    pub kind: UploadKind,
    /// `cover` or `contain`.
    pub fit: String,
    pub source_width: u32,
    pub source_height: u32,
    /// Always the panel size; the upload contract has no other.
    pub output_width: u32,
    pub output_height: u32,
    pub frames: usize,
    pub total_bytes: usize,
    pub chunk_count: usize,
    pub header_count: usize,
    pub command_count: usize,
    pub operations: usize,
    pub rotation: u8,
    pub duration_ms: u64,
    pub content_id: String,
    /// A PNG data URL of the first frame as encoded, or `null` if the thumbnail failed.
    pub preview: Option<String>,
    pub commands: Vec<PlanCommandSummary>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_defaults_match_the_ui() {
        let settings = SettingsState::default();
        assert!(settings.screen_on);
        assert_eq!(settings.brightness, 70);
        assert!(settings.idle_screen);
        assert!(!settings.rgb_sync);
        assert!(settings.record_cpu_clock);
        assert!(settings.record_cpu_temp);
        assert_eq!(settings.screen_restore_brightness, 70);
        assert_eq!(settings.main_area, 0);
        assert_eq!(settings.style_area, 0);
        assert_eq!(settings.aux_area, 0);
        assert_eq!(settings.media_seconds, 3);
        assert_eq!(settings.media_animation_name, "Split Display");
        assert_eq!(settings.cpu_label, "--");
    }

    #[test]
    fn older_settings_payloads_get_the_documented_defaults() {
        let mut json = serde_json::to_value(SettingsState::default()).unwrap();
        let object = json.as_object_mut().unwrap();
        object.remove("idle_screen");
        object.remove("rgb_sync");
        object.remove("record_cpu_clock");
        object.remove("record_cpu_temp");
        let decoded: SettingsState = serde_json::from_value(json).unwrap();
        assert!(decoded.idle_screen);
        assert!(!decoded.rgb_sync);
        assert!(decoded.record_cpu_clock);
        assert!(decoded.record_cpu_temp);
    }

    #[test]
    fn status_flattens_the_device_state() {
        let status = StatusDto {
            device: DeviceState {
                device_open: true,
                device_present: true,
                service_running: false,
                service_supported: true,
                service_output: "STOPPED".to_owned(),
                last_error: None,
                rotation: Some(2),
                settings: SettingsState::default(),
                telemetry: None,
            },
            rotation_source: "0x10 reply (advisory)".to_owned(),
            rotation_degrees: Some(180),
            mode: Some("machine-info".to_owned()),
            mode_raw: Some(0xFF),
            result: Some("success".to_owned()),
            packet_hex: Some("AA 2E 10".to_owned()),
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["device_open"], true);
        assert_eq!(json["rotation_source"], "0x10 reply (advisory)");
        assert_eq!(json["settings"]["brightness"], 70);
        assert!(json.get("device").is_none());
        assert!(json.get("telemetry").is_none());
        let decoded: StatusDto = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, status);
    }

    #[test]
    fn upload_kinds_are_lowercase_on_the_wire() {
        assert_eq!(serde_json::to_value(UploadKind::Image).unwrap(), "image");
        assert_eq!(serde_json::to_value(UploadKind::Gif).unwrap(), "gif");
        assert_eq!(
            serde_json::from_value::<UploadKind>("gif".into()).unwrap(),
            UploadKind::Gif
        );
    }

    #[test]
    fn a_device_state_from_the_ui_without_telemetry_still_parses() {
        let json = serde_json::json!({
            "device_open": false,
            "device_present": true,
            "service_running": true,
            "service_supported": true,
            "service_output": "",
            "last_error": null,
            "rotation": null,
            "settings": SettingsState::default()
        });
        let state: DeviceState = serde_json::from_value(json).unwrap();
        assert!(state.telemetry.is_none());
    }
}
