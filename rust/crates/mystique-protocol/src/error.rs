use thiserror::Error;

use crate::result_code::ResultCode;

/// Everything that can be wrong with a packet or a reply.
///
/// "Malformed" and "the device said no" are deliberately different variants:
/// a [`ProtocolError::DeviceRejected`] means the envelope was valid and the device
/// answered with a non-success result byte, while every other variant means the
/// bytes themselves could not be trusted.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProtocolError {
    #[error("payload is {actual} bytes; the maximum is {max}")]
    PayloadTooLong { actual: usize, max: usize },

    #[error("command 0x{command:02X} requires a non-empty payload; empty is a silent no-op")]
    PayloadRequired { command: u8 },

    #[error("response is {actual} bytes; at least {need} are required")]
    Truncated { actual: usize, need: usize },

    #[error("bad response header: {0:02X?} (expected 55 2E)")]
    BadHeader([u8; 2]),

    #[error("bad response trailer at bytes 42..46: {0:02X?} (expected 48 49 44 43)")]
    BadTrailer([u8; 4]),

    #[error("bad response checksum: got 0x{got:04X}, computed 0x{expected:04X}")]
    BadChecksum { got: u16, expected: u16 },

    #[error("command echo mismatch: sent 0x{sent:02X}, received 0x{received:02X}")]
    EchoMismatch { sent: u8, received: u8 },

    #[error("device rejected command 0x{command:02X}: {result}")]
    DeviceRejected { command: u8, result: ResultCode },

    #[error("rotation {0} is out of range; expected 0, 1, 2 or 3")]
    RotationOutOfRange(u8),

    #[error("brightness {0} is out of range; expected 0..=100")]
    BrightnessOutOfRange(u8),

    #[error("display area selector 0x{0:02X} is above the maximum 0x0F")]
    DisplayAreaOutOfRange(u8),

    #[error("style type {0} is out of range; expected 0 Normal, 1 Ring or 2 Stopwatch")]
    DisplayStyleOutOfRange(u8),

    #[error("media timing value {0} is out of range; expected 0, 1 or 2")]
    MediaTimingOutOfRange(u8),

    #[error("device info reply is {0} bytes; expected 48")]
    InfoLength(usize),

    #[error("image chunk is empty")]
    EmptyImageChunk,

    #[error("image chunk is {actual} bytes; the maximum is {max}")]
    ImageChunkTooLong { actual: usize, max: usize },
}
