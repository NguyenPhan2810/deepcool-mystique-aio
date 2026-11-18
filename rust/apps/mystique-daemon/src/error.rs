//! Maps protocol, USB, media and filesystem failures onto the shared
//! [`CommandError`], and builds the packet records that make the wire visible.
//!
//! The codes are the Tauri app's, so a client can keep one error renderer. Hints are
//! written for a daemon: they say what the daemon will do next, not what button to
//! press.

use std::path::Path;

use mystique_ipc::{codes, hex, CommandError, PacketRecord};
use mystique_media::MediaError;
use mystique_protocol::{Command, CommandPacket, ProtocolError};
use mystique_usb::{DeviceError, Transfer};

/// One command packet, recorded before the write so a failed write still reports the
/// bytes that were attempted.
pub fn packet_record(packet: &CommandPacket) -> Result<PacketRecord, CommandError> {
    let bytes = packet.packet().map_err(from_protocol)?;
    Ok(PacketRecord {
        label: command_label(packet.command),
        code: Some(packet.code()),
        pipe: packet.pipe().label().to_owned(),
        payload_hex: hex(&packet.payload),
        packet_hex: hex(&bytes),
        result: "not sent".to_owned(),
        reply_hex: None,
    })
}

/// A frame header or the `dcldfinish` terminator: no command byte, no reply.
pub fn raw_record(label: String, packet_hex: String, pipe: &str) -> PacketRecord {
    PacketRecord {
        label,
        code: None,
        pipe: pipe.to_owned(),
        payload_hex: String::new(),
        packet_hex,
        result: "not sent".to_owned(),
        reply_hex: None,
    }
}

/// `0x02 display-config`, using the protocol crate's documented name for the byte.
pub fn command_label(command: Command) -> String {
    match mystique_protocol::commands::find(command.code()) {
        Some(info) => format!("0x{:02X} {}", info.code, info.name),
        None => format!("0x{:02X}", command.code()),
    }
}

pub fn from_protocol(error: ProtocolError) -> CommandError {
    CommandError::new(protocol_code(&error), error.to_string())
}

pub fn from_device(error: DeviceError) -> CommandError {
    let hint = match &error {
        DeviceError::NotFound => {
            Some("the daemon keeps retrying; plug the panel in when it is available")
        }
        DeviceError::AccessDenied => Some(
            "another process owns the interface: stop MystiqueService and close DeepCool.exe. \
             The daemon retries once the interface is free.",
        ),
        DeviceError::Timeout {
            during: Transfer::Write,
        } => Some(
            "a blocked write is the wedge signature. The daemon drops the handle and backs off; \
             a wedged pipe may need a PnP restart or a replug.",
        ),
        DeviceError::BadCommand { .. } => {
            Some("recoverable: the daemon closes the handle and re-runs init after a pause")
        }
        DeviceError::Disconnected => {
            Some("the handle was dropped; the daemon retries and re-runs init on reacquire")
        }
        _ => None,
    };
    let code = match &error {
        DeviceError::NotFound => codes::NOT_FOUND,
        DeviceError::AccessDenied => codes::ACCESS_DENIED,
        DeviceError::Timeout { .. } => codes::TIMEOUT,
        DeviceError::Stall { .. } => codes::STALL,
        DeviceError::BadCommand { .. } => codes::BAD_COMMAND,
        DeviceError::Disconnected => codes::DISCONNECTED,
        DeviceError::Protocol(inner) => protocol_code(inner),
        DeviceError::Rejected(_) => codes::REJECTED,
        DeviceError::Other { .. } => "usb_other",
    };
    CommandError {
        code: code.to_owned(),
        message: error.to_string(),
        hint: hint.map(str::to_owned),
        packets: Vec::new(),
    }
}

pub fn from_media(error: MediaError) -> CommandError {
    let hint = match &error {
        MediaError::Unsupported { .. } => Some("choose a PNG, JPEG or GIF file"),
        MediaError::NoFrames { .. } => Some("the GIF has no frames to send"),
        _ => None,
    };
    let error = CommandError::new(codes::MEDIA, error.to_string());
    match hint {
        Some(hint) => error.with_hint(hint),
        None => error,
    }
}

pub fn read_error(path: &Path, source: std::io::Error) -> CommandError {
    CommandError::new(codes::IO, format!("reading {}: {source}", path.display()))
}

/// True when the code means the link itself failed, so the handle must be dropped and
/// reacquired. A device **rejection** is not a lost device: it answered, so the link is
/// kept and the error is shown in place. Range, protocol and media errors are about a
/// request, not the device.
pub fn should_reacquire(code: &str) -> bool {
    matches!(
        code,
        codes::NOT_FOUND
            | codes::ACCESS_DENIED
            | codes::TIMEOUT
            | codes::STALL
            | codes::BAD_COMMAND
            | codes::DISCONNECTED
            | "usb_other"
    )
}

fn protocol_code(error: &ProtocolError) -> &'static str {
    match error {
        ProtocolError::RotationOutOfRange(_)
        | ProtocolError::BrightnessOutOfRange(_)
        | ProtocolError::DisplayAreaOutOfRange(_)
        | ProtocolError::DisplayStyleOutOfRange(_)
        | ProtocolError::MediaTimingOutOfRange(_)
        | ProtocolError::PayloadTooLong { .. }
        | ProtocolError::ImageChunkTooLong { .. } => codes::RANGE,
        _ => codes::PROTOCOL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mystique_protocol::Rotation;

    #[test]
    fn a_packet_record_carries_the_exact_bytes() {
        let packet = mystique_protocol::set_brightness(50).unwrap();
        let record = packet_record(&packet).unwrap();
        assert_eq!(record.label, "0x02 display-config");
        // Byte 3 is `00` because RGB sync is on in the default config, and the panel
        // follows the motherboard at `00`.
        assert_eq!(record.payload_hex, "01 00 00 00 32");
        assert_eq!(record.code, Some(0x02));
        assert_eq!(record.pipe, "init");
        assert!(record.packet_hex.starts_with("AA 2E 02 01 00 00 00 32"));
        assert_eq!(record.packet_hex.split(' ').count(), 48);
    }

    #[test]
    fn out_of_range_is_a_range_code() {
        let error = from_protocol(ProtocolError::RotationOutOfRange(9));
        assert_eq!(error.code, "range");
        let rotation = Rotation::new(9).unwrap_err();
        assert_eq!(from_protocol(rotation).code, "range");
    }

    #[test]
    fn a_missing_device_keeps_retrying_and_says_so() {
        let error = from_device(DeviceError::NotFound);
        assert_eq!(error.code, "not_found");
        assert!(error.hint.unwrap_or_default().contains("retrying"));
        assert!(error.packets.is_empty());
    }

    #[test]
    fn a_write_timeout_is_the_wedge_signature() {
        let error = from_device(DeviceError::Timeout {
            during: Transfer::Write,
        });
        assert_eq!(error.code, "timeout");
        assert!(error.hint.unwrap_or_default().contains("wedge"));
    }

    #[test]
    fn a_disconnect_has_a_daemon_hint() {
        let error = from_device(DeviceError::Disconnected);
        assert_eq!(error.code, "disconnected");
        assert!(error.hint.unwrap_or_default().contains("retries"));
    }
}
