//! `0x03` — the display-target selector.
//!
//! Four labelled vendor transitions pinned this (`re/findings/17-mode-and-idle-screen.md`):
//! `01` Monitor, `02` Media, `03` Recording. The init chain writes `01` because the
//! vendor does; that is the same command with a different payload, not a different step.
//!
//! Stored media still dominates the panel while it exists: selecting Monitor does not
//! erase an uploaded image. `0x14` does that (`re/findings/15-driver-verification.md`).
//! The selector chooses what the panel draws when there is no stored content.

use crate::commands::Command;
use crate::init::SETTLE_STEP;
use crate::upload::CommandPacket;

/// Payload length of `0x03`: one selector byte.
pub const DISPLAY_TARGET_LEN: usize = 1;

/// The three targets the vendor's mode dropdown selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayTarget {
    Monitor,
    Media,
    Recording,
}

impl DisplayTarget {
    pub const MONITOR_RAW: u8 = 0x01;
    pub const MEDIA_RAW: u8 = 0x02;
    pub const RECORDING_RAW: u8 = 0x03;

    pub const fn raw(self) -> u8 {
        match self {
            Self::Monitor => Self::MONITOR_RAW,
            Self::Media => Self::MEDIA_RAW,
            Self::Recording => Self::RECORDING_RAW,
        }
    }

    pub const fn from_raw(raw: u8) -> Option<Self> {
        match raw {
            Self::MONITOR_RAW => Some(Self::Monitor),
            Self::MEDIA_RAW => Some(Self::Media),
            Self::RECORDING_RAW => Some(Self::Recording),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Monitor => "monitor",
            Self::Media => "media",
            Self::Recording => "recording",
        }
    }

    pub fn packet(self) -> CommandPacket {
        CommandPacket::new(Command::SelectMode, vec![self.raw()], SETTLE_STEP)
    }
}

/// One `0x03` write. The vendor treats it as a switch, not a toggle: `[01]` always means
/// Monitor, so the whole payload is the target.
pub fn set_mode(target: DisplayTarget) -> CommandPacket {
    target.packet()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::Pipe;

    #[test]
    fn the_capture_values_are_pinned() {
        assert_eq!(DisplayTarget::Monitor.raw(), 0x01);
        assert_eq!(DisplayTarget::Media.raw(), 0x02);
        assert_eq!(DisplayTarget::Recording.raw(), 0x03);
    }

    #[test]
    fn every_target_round_trips_and_nothing_else_does() {
        for target in [
            DisplayTarget::Monitor,
            DisplayTarget::Media,
            DisplayTarget::Recording,
        ] {
            assert_eq!(DisplayTarget::from_raw(target.raw()), Some(target));
        }
        for raw in [0x00u8, 0x04, 0xFF] {
            assert_eq!(DisplayTarget::from_raw(raw), None);
        }
    }

    #[test]
    fn the_packet_is_one_byte_on_the_init_pipe() {
        let packet = set_mode(DisplayTarget::Recording);
        assert_eq!(packet.command, Command::SelectMode);
        assert_eq!(packet.payload, vec![0x03]);
        assert_eq!(packet.code(), 0x03);
        assert_eq!(packet.pipe(), Pipe::Init);
        let bytes = packet.packet().unwrap();
        assert_eq!(bytes[2], 0x03);
        assert_eq!(bytes[3], 0x03);
        assert_eq!(bytes.len(), crate::packet::PACKET_LEN);
    }
}
