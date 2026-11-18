//! The 48-byte vendor packet, both directions.

use crate::error::ProtocolError;

/// Outbound request length. Verified: every request the working implementation
/// sends, and every request in `re/fixtures/`, is exactly 48 bytes.
pub const PACKET_LEN: usize = 48;

/// The reply length actually transferred when a 64-byte buffer is offered.
/// Verified: no reply longer than 48 bytes has ever been observed; the extra
/// 16 bytes of the buffer stay untouched.
pub const RESPONSE_LEN: usize = 48;

/// Read buffer size used by the reference implementation.
pub const READ_BUFFER_LEN: usize = 64;

/// Maximum payload that fits between the 3-byte prefix and the 4-byte trailer.
pub const MAX_PAYLOAD: usize = 39;

/// Request prefix. A wrong first byte produces **no reply at all** and wedged the
/// device for ~24 minutes during capture (`re/findings/03-live-capture.md` §9), so
/// [`build`] is the only supported way to make a packet.
pub const REQUEST_HEADER: [u8; 2] = [0xAA, 0x2E];

/// Reply prefix.
pub const RESPONSE_HEADER: [u8; 2] = [0x55, 0x2E];

/// The ASCII trailer at bytes 42..46, in both directions. Verified: the device
/// validates it independently of the checksum and answers `0x01` when it is wrong.
pub const TRAILER: [u8; 4] = *b"HIDC";

pub const TRAILER_OFFSET: usize = 42;
pub const CHECKSUM_OFFSET: usize = 46;

/// Number of leading bytes covered by the checksum: 0..=45, i.e. including `HIDC`.
/// Verified: excluding `HIDC` from the sum makes the device answer `0x02`.
pub const CHECKSUM_COVERAGE: usize = 46;

/// Which endpoint pair a command belongs on.
///
/// The split is part of a command's identity, not a global preference: the
/// 2026-11-18 sweep (`re/findings/11-sweep-results.md`) found 18 bytes echoed on
/// the init pipe, only `0x01` and `0x10` on the data pipe, and `0x0A` gated on a
/// non-empty payload there. [`crate::commands::Command`] carries this routing so a
/// caller cannot choose the wrong pipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pipe {
    /// OUT `0x01`, IN `0x81` — initialization and identity.
    Init,
    /// OUT `0x02`, IN `0x82` — status and telemetry.
    Data,
}

impl Pipe {
    pub const fn out_endpoint(self) -> u8 {
        match self {
            Self::Init => 0x01,
            Self::Data => 0x02,
        }
    }

    pub const fn in_endpoint(self) -> u8 {
        match self {
            Self::Init => 0x81,
            Self::Data => 0x82,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::Data => "data",
        }
    }
}

/// Sum of `bytes` modulo 2^16.
///
/// This is the only checksum the device uses: the 48-byte packet at 46..48, and the
/// `DCLd` frame header at 62..64. No CRC anywhere in the verified protocol.
pub fn checksum16(bytes: &[u8]) -> u16 {
    bytes
        .iter()
        .fold(0u16, |sum, byte| sum.wrapping_add(u16::from(*byte)))
}

/// Builds a 48-byte request.
///
/// The trailer and checksum are always written by this function; callers cannot
/// produce a packet with a bad marker or a stale sum.
pub fn build(command: u8, payload: &[u8]) -> Result<[u8; PACKET_LEN], ProtocolError> {
    if payload.len() > MAX_PAYLOAD {
        return Err(ProtocolError::PayloadTooLong {
            actual: payload.len(),
            max: MAX_PAYLOAD,
        });
    }

    let mut packet = [0u8; PACKET_LEN];
    packet[0] = REQUEST_HEADER[0];
    packet[1] = REQUEST_HEADER[1];
    packet[2] = command;
    packet[3..3 + payload.len()].copy_from_slice(payload);
    packet[TRAILER_OFFSET..TRAILER_OFFSET + 4].copy_from_slice(&TRAILER);

    let sum = checksum16(&packet[..CHECKSUM_COVERAGE]);
    packet[CHECKSUM_OFFSET..CHECKSUM_OFFSET + 2].copy_from_slice(&sum.to_le_bytes());
    Ok(packet)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_status_request_matches_the_captured_bytes() {
        let packet = build(0x10, &[]).unwrap();
        let expected = "AA 2E 10 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 \
             00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 48 49 44 43 00 02";
        assert_eq!(crate::tests::hex(packet.as_slice()), expected);
    }

    #[test]
    fn checksum_covers_hidc() {
        let packet = build(0x10, &[]).unwrap();
        assert_eq!(checksum16(&packet[..46]), 0x0200);
        assert_eq!(&packet[46..48], &[0x00, 0x02]);
        // Excluding HIDC gives a different value; this is the experiment that proved
        // coverage (re/fixtures/07-checksum-covers-hidc.json).
        assert_ne!(checksum16(&packet[..42]), 0x0200);
    }

    #[test]
    fn rejects_oversized_payloads() {
        let error = build(0x01, &[0u8; MAX_PAYLOAD + 1]).unwrap_err();
        assert_eq!(
            error,
            ProtocolError::PayloadTooLong {
                actual: 40,
                max: 39
            }
        );
    }

    #[test]
    fn accepts_a_full_payload() {
        let packet = build(0x01, &[0xAB; MAX_PAYLOAD]).unwrap();
        assert_eq!(packet[3], 0xAB);
        assert_eq!(packet[41], 0xAB);
        assert_eq!(&packet[42..46], b"HIDC");
    }

    #[test]
    fn checksum_wraps_at_sixteen_bits() {
        assert_eq!(checksum16(&[0xFF; 300]), u16::wrapping_mul(0x00FF, 300));
    }

    #[test]
    fn pipe_endpoints_match_the_descriptors() {
        assert_eq!(
            (Pipe::Init.out_endpoint(), Pipe::Init.in_endpoint()),
            (0x01, 0x81)
        );
        assert_eq!(
            (Pipe::Data.out_endpoint(), Pipe::Data.in_endpoint()),
            (0x02, 0x82)
        );
    }
}
