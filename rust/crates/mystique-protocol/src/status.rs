//! Status — command `0x10`. **Advisory only.**

use std::fmt;

use crate::error::ProtocolError;
use crate::response::Response;
use crate::result_code::ResultCode;

/// What `0x10` byte 5 said. Verified values: `0xFF` in Machine Info mode and `0x00`
/// in image/GIF mode. A capture showed `0x00` while the panel was visibly in Machine
/// Info mode, so this must never gate anything.
///
/// This is the **read** side and is advisory. There is no verified write-side selector:
/// `0x03` is **Unknown**, and media is content presence — erasing stored images with
/// `0x14` is what returns the panel to machine info.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayMode {
    MachineInfo,
    ImageOrGif,
    Unknown,
}

impl DisplayMode {
    pub const fn from_raw(raw: u8) -> Self {
        match raw {
            0xFF => Self::MachineInfo,
            0x00 => Self::ImageOrGif,
            _ => Self::Unknown,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::MachineInfo => "machine-info",
            Self::ImageOrGif => "image-or-gif",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for DisplayMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The decoded `0x10` reply.
///
/// Parsing never judges the mode, the rotation or the result code: the caller only
/// logs them. In particular a `0x02` result still carries rotation and mode on this
/// device (`re/fixtures/08-status-checksum-mismatch.json`), and `0x00` was observed
/// while the panel was live in Machine Info mode. **Do not fail init on this.**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelStatus {
    /// Raw byte 4. `0..=3` on this unit.
    pub rotation: u8,
    /// Raw byte 5.
    pub raw_mode: u8,
    pub mode: DisplayMode,
    /// The device's result byte. Not a gate.
    pub result: ResultCode,
}

impl PanelStatus {
    /// Rotation in degrees, or `None` if the device sent a value above 3.
    pub const fn rotation_degrees(&self) -> Option<u16> {
        if self.rotation <= 3 {
            Some(self.rotation as u16 * 90)
        } else {
            None
        }
    }

    /// True when the device says it is in Machine Info mode. Advisory.
    pub const fn is_machine_info(&self) -> bool {
        matches!(self.mode, DisplayMode::MachineInfo)
    }
}

/// Extracts rotation and mode from a parsed reply. Never fails on payload values;
/// only the envelope (already validated by [`Response::parse`]) can be wrong.
pub fn parse_status(response: &Response) -> PanelStatus {
    let rotation = response.as_bytes()[4];
    let raw_mode = response.as_bytes()[5];
    PanelStatus {
        rotation,
        raw_mode,
        mode: DisplayMode::from_raw(raw_mode),
        result: response.result(),
    }
}

impl TryFrom<&Response> for PanelStatus {
    type Error = ProtocolError;

    /// Convenience for callers that want the echo checked but the result ignored.
    fn try_from(response: &Response) -> Result<Self, Self::Error> {
        response.check_echo(crate::commands::CMD_STATUS)?;
        Ok(parse_status(response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::checksum16;

    fn reply(result: u8, rotation: u8, mode: u8) -> Response {
        let mut raw = [0u8; 48];
        raw[0] = 0x55;
        raw[1] = 0x2E;
        raw[2] = 0x10;
        raw[3] = result;
        raw[4] = rotation;
        raw[5] = mode;
        raw[42..46].copy_from_slice(b"HIDC");
        let sum = checksum16(&raw[..46]);
        raw[46..48].copy_from_slice(&sum.to_le_bytes());
        Response::parse(&raw).unwrap()
    }

    #[test]
    fn reads_machine_info() {
        let status = parse_status(&reply(0x00, 0x00, 0xFF));
        assert_eq!(status.rotation, 0);
        assert_eq!(status.mode, DisplayMode::MachineInfo);
        assert!(status.is_machine_info());
        assert_eq!(status.rotation_degrees(), Some(0));
    }

    #[test]
    fn does_not_fail_when_the_device_rejects_the_checksum() {
        // Fixture 08: result 0x02 but rotation/mode are still present.
        let status = parse_status(&reply(0x02, 0x00, 0xFF));
        assert_eq!(status.result, ResultCode::BadChecksum);
        assert_eq!(status.mode, DisplayMode::MachineInfo);
        assert_eq!(status.rotation, 0);
    }

    #[test]
    fn a_zero_mode_is_reported_but_is_not_a_gate() {
        let status = parse_status(&reply(0x00, 0x00, 0x00));
        assert_eq!(status.mode, DisplayMode::ImageOrGif);
        assert!(!status.is_machine_info());
        // Parsing succeeded; the caller decides what to log.
    }

    #[test]
    fn unknown_modes_and_rotations_do_not_panic() {
        let status = parse_status(&reply(0x00, 9, 0x42));
        assert_eq!(status.mode, DisplayMode::Unknown);
        assert_eq!(status.rotation_degrees(), None);
    }
}
