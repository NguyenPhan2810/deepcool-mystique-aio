//! Pure wire-format layer for the DeepCool MYSTIQUE AIO LCD (VID:PID `3633:0009`).
//!
//! This crate performs no I/O and contains no OS calls. It builds and parses the
//! 48-byte vendor packets, the 64-byte `DCLd` frame header, and the ordered
//! operation plans that a transport executes.
//!
//! # Evidence
//!
//! Every constant here is either:
//!
//! - **Verified** — observed on the real device and recorded in `re/fixtures/` or
//!   `re/reports/`, or
//! - **Inferred** — recovered from vendor code or third-party prior art and marked as
//!   such in a doc comment, or
//! - **Unknown** — present because the device uses it, with no invented meaning.
//!
//! Nothing in this crate may be used to flash firmware.

#![forbid(unsafe_code)]

pub mod commands;
pub mod dcl;
pub mod display;
pub mod erase;
pub mod error;
pub mod info;
pub mod init;
pub mod packet;
pub mod response;
pub mod result_code;
pub mod rotation;
pub mod status;
pub mod telemetry;
pub mod upload;

pub use commands::{
    label_payload, mode_switch_payload, time_payload, Command, CommandInfo, Evidence,
    ObservedCommand, COMMANDS, LABEL_MAX_LEN, MODE_SWITCH_PREFIX, OBSERVED_COMMANDS, TIME_HEADER,
    TIME_PAYLOAD_LEN,
};
pub use dcl::{
    build_header as build_dcld_header, ContentId, DCLD_HEADER_LEN, DCLD_KIND_ANIMATED,
    DCLD_KIND_STATIC, FINISH_LEN, FINISH_MAGIC,
};
pub use display::{
    rotate_with_gyro_off, set_brightness, set_display_areas, set_gyro, set_idle_screen,
    set_media_timing, set_mode, set_recording, set_rgb_sync, set_rotation, DisplayAreas,
    DisplayConfig, DisplayTarget, MediaTiming, RecordingConfig, BRIGHTNESS_MAX, DISPLAY_AREAS_LEN,
    DISPLAY_AREA_MAX, DISPLAY_STYLE_MAX, DISPLAY_TARGET_LEN, MEDIA_TIMING_LEN, MEDIA_TIMING_MAX,
    RECORDING_OPTIONS_LEN, STYLE_NORMAL, STYLE_RING, STYLE_STOPWATCH,
};
pub use erase::erase_stored_images;
pub use error::ProtocolError;
pub use info::DeviceInfo;
pub use init::{
    machine_info_sequence_full, machine_info_sequence_with, InitStep, MachineLabels,
    SETTLE_MODE_SWITCH, SETTLE_STEP,
};
pub use packet::{
    build as build_packet, checksum16, Pipe, MAX_PAYLOAD, PACKET_LEN, READ_BUFFER_LEN, RESPONSE_LEN,
};
pub use response::Response;
pub use result_code::ResultCode;
pub use rotation::Rotation;
pub use status::{parse_status, DisplayMode, PanelStatus};
pub use telemetry::TelemetryFrame;
pub use upload::{
    build_upload_plan, upload_preamble, upload_preamble_with, validate_plan, CommandPacket,
    FinishPacket, Operation, UploadError, UploadFrame, UploadRequest, IMAGE_CHUNK_LEN,
};

#[cfg(test)]
pub(crate) mod tests {
    /// Uppercase, space-separated hex, the spelling the fixtures use.
    pub fn hex(bytes: &[u8]) -> String {
        bytes
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Parses a fixture hex string, tolerating spaces and newlines.
    pub fn unhex(text: &str) -> Vec<u8> {
        let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            compact.chars().all(|c| c.is_ascii_hexdigit()),
            "non-hex fixture input: {compact}"
        );
        assert_eq!(compact.len() % 2, 0, "odd hex length: {compact}");
        (0..compact.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&compact[index..index + 2], 16).unwrap())
            .collect()
    }
}
