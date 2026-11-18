//! `0x14` — erase the images stored on the device, as a standalone command.
//!
//! Presence is **Verified**: the 360 sweep echoed `0x14` on the init pipe, and the
//! upload capture contains it once, between uploads, with an empty payload
//! (`re/findings/11-sweep-results.md`, `re/findings/06-image-upload-protocol.md`).
//!
//! The effect was **Verified on hardware on 2026-11-18**
//! (`re/findings/15-driver-verification.md`): after an upload held the panel on the
//! stored content, `0x03 [01]` plus the handshake did not bring it back, and `0x14`
//! alone did. Media is content presence; erase is how the panel returns to machine info.
//!
//! This module exposes the command and nothing more. Erasing is destructive and the
//! driver has no read-back, so callers must warn before sending it.

use crate::commands::Command;
use crate::init::SETTLE_STEP;
use crate::upload::CommandPacket;

/// Builds `0x14` with the captured empty payload, routed to the init pipe.
///
/// The typed [`Command`] decides the pipe, so this packet cannot be sent on the data
/// pipe where the device would answer with a neutral ACK and do nothing.
pub fn erase_stored_images() -> CommandPacket {
    CommandPacket::new(Command::EraseImages, [], SETTLE_STEP)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::Pipe;

    #[test]
    fn erase_is_0x14_empty_on_the_init_pipe() {
        let packet = erase_stored_images();
        assert_eq!(packet.command, Command::EraseImages);
        assert_eq!(packet.code(), 0x14);
        assert_eq!(packet.pipe(), Pipe::Init);
        assert!(packet.payload.is_empty());
        let bytes = packet.packet().unwrap();
        assert_eq!(&bytes[..3], &[0xAA, 0x2E, 0x14]);
        assert_eq!(bytes.len(), crate::packet::PACKET_LEN);
    }

    #[test]
    fn erase_is_not_a_0x03_selector() {
        // The old mode write was 0x03, whose meaning is unknown. Erase is a separate
        // action and cannot quietly acquire a selector byte.
        let packet = erase_stored_images();
        assert_ne!(packet.command, Command::InitStep03);
        assert_ne!(packet.code(), 0x03);
    }
}
