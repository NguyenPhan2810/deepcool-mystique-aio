//! Parsing and redaction of the 48-byte reply.

use std::fmt;

use crate::error::ProtocolError;
use crate::info::looks_like_identifier;
use crate::packet::{
    checksum16, CHECKSUM_COVERAGE, CHECKSUM_OFFSET, RESPONSE_HEADER, RESPONSE_LEN, TRAILER,
    TRAILER_OFFSET,
};
use crate::result_code::ResultCode;

/// A validated reply envelope.
///
/// Construction checks, in this order: length, header, trailer, checksum. The
/// command echo and the result code are *not* judged here, because both are data
/// the protocol mapper needs to record even when they are wrong.
#[derive(Clone, PartialEq, Eq)]
pub struct Response {
    bytes: [u8; RESPONSE_LEN],
    wire_len: usize,
}

impl Response {
    /// Parses the first 48 bytes of a reply.
    ///
    /// A longer buffer is accepted and the extra bytes are ignored — the reference
    /// implementation reads into 64 bytes and the device transfers 48.
    pub fn parse(raw: &[u8]) -> Result<Self, ProtocolError> {
        if raw.len() < RESPONSE_LEN {
            return Err(ProtocolError::Truncated {
                actual: raw.len(),
                need: RESPONSE_LEN,
            });
        }

        let mut bytes = [0u8; RESPONSE_LEN];
        bytes.copy_from_slice(&raw[..RESPONSE_LEN]);

        if bytes[..2] != RESPONSE_HEADER {
            return Err(ProtocolError::BadHeader([bytes[0], bytes[1]]));
        }

        let mut trailer = [0u8; 4];
        trailer.copy_from_slice(&bytes[TRAILER_OFFSET..TRAILER_OFFSET + 4]);
        if trailer != TRAILER {
            return Err(ProtocolError::BadTrailer(trailer));
        }

        let expected = checksum16(&bytes[..CHECKSUM_COVERAGE]);
        let got = u16::from_le_bytes([bytes[CHECKSUM_OFFSET], bytes[CHECKSUM_OFFSET + 1]]);
        if got != expected {
            return Err(ProtocolError::BadChecksum { got, expected });
        }

        Ok(Self {
            bytes,
            wire_len: raw.len(),
        })
    }

    /// The 48 parsed bytes, trailer and checksum included.
    pub const fn as_bytes(&self) -> &[u8; RESPONSE_LEN] {
        &self.bytes
    }

    /// Bytes actually transferred. Normally 48.
    pub const fn wire_len(&self) -> usize {
        self.wire_len
    }

    /// Response byte 2: the command, or `0x00`.
    ///
    /// Verified: `0x15`, `0x16`, `0x17` and `0x0A` reply with `0x00` instead of
    /// their own command byte, so both are valid.
    pub const fn command_echo(&self) -> u8 {
        self.bytes[2]
    }

    /// Response byte 3.
    pub const fn result(&self) -> ResultCode {
        ResultCode::from_raw(self.bytes[3])
    }

    /// Bytes 4..42. The identifier lives at 4..36 of a `0x12` reply; for every other
    /// captured command the whole body is zero.
    pub fn body(&self) -> &[u8] {
        &self.bytes[4..42]
    }

    /// Accepts the echo if it is either the command or `0x00`.
    pub fn check_echo(&self, command: u8) -> Result<(), ProtocolError> {
        let received = self.command_echo();
        if received == command || received == 0x00 {
            Ok(())
        } else {
            Err(ProtocolError::EchoMismatch {
                sent: command,
                received,
            })
        }
    }

    /// Validates the echo and requires result `0x00`.
    pub fn require_success(&self, command: u8) -> Result<(), ProtocolError> {
        self.check_echo(command)?;
        if self.result().is_ok() {
            Ok(())
        } else {
            Err(ProtocolError::DeviceRejected {
                command,
                result: self.result(),
            })
        }
    }

    /// Hex dump with the device identifier masked.
    ///
    /// Masking is applied when the reply is a `0x12` answer or when bytes 4..42 have
    /// the exact shape of the identifier (32 uppercase hex characters followed by six
    /// zero bytes). Bytes 36..42 stay visible because they are proven zero padding.
    /// See `re/findings/03-live-capture.md` §3.1.
    pub fn redacted_hex(&self) -> String {
        let mask = self.command_echo() == crate::commands::CMD_DEVICE_INFO
            || looks_like_identifier(&self.bytes[4..42]);

        let mut out = String::with_capacity(RESPONSE_LEN * 3);
        for (index, byte) in self.bytes.iter().enumerate() {
            if index > 0 {
                out.push(' ');
            }
            if mask && (4..36).contains(&index) {
                out.push_str("..");
            } else {
                out.push_str(&format!("{byte:02X}"));
            }
        }
        out
    }
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Response")
            .field(
                "command_echo",
                &format_args!("0x{:02X}", self.command_echo()),
            )
            .field("result", &self.result())
            .field("wire_len", &self.wire_len)
            .field("hex_redacted", &self.redacted_hex())
            .finish()
    }
}

impl fmt::Display for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.redacted_hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `0x10` in Machine Info mode, byte-exact from
    /// `re/fixtures/02-status-machine-info-0x10.json`.
    const STATUS: &str = "55 2E 10 00 00 FF 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 \
                          00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 48 49 44 43 AA 02";

    /// `0x02` ACK, byte-exact from `re/fixtures/03-init-sequence.json`.
    const ACK_02: &str = "55 2E 02 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 \
                          00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 48 49 44 43 9D 01";

    fn parse(text: &str) -> Response {
        Response::parse(&crate::tests::unhex(text)).unwrap()
    }

    fn reseal(raw: &mut [u8]) {
        let sum = checksum16(&raw[..46]);
        raw[46..48].copy_from_slice(&sum.to_le_bytes());
    }

    #[test]
    fn parses_a_captured_reply() {
        let response = parse(STATUS);
        assert_eq!(response.command_echo(), 0x10);
        assert_eq!(response.result(), ResultCode::Ok);
        assert_eq!(response.body()[0], 0x00); // rotation
        assert_eq!(response.body()[1], 0xFF); // mode
        assert_eq!(response.wire_len(), 48);
    }

    #[test]
    fn accepts_a_six_y_byte_buffer_with_trailing_garbage() {
        let mut raw = crate::tests::unhex(ACK_02);
        raw.extend_from_slice(&[0xEE; 16]);
        let response = Response::parse(&raw).unwrap();
        assert_eq!(response.wire_len(), 64);
        assert_eq!(response.result(), ResultCode::Ok);
    }

    #[test]
    fn rejects_truncated_replies() {
        let raw = crate::tests::unhex(STATUS);
        let error = Response::parse(&raw[..47]).unwrap_err();
        assert_eq!(
            error,
            ProtocolError::Truncated {
                actual: 47,
                need: 48
            }
        );
        assert!(matches!(
            Response::parse(&raw[..3]).unwrap_err(),
            ProtocolError::Truncated { actual: 3, .. }
        ));
        assert!(Response::parse(&[]).is_err());
    }

    #[test]
    fn rejects_a_bad_header() {
        let mut raw = crate::tests::unhex(STATUS);
        raw[0] = 0xAB;
        assert_eq!(
            Response::parse(&raw).unwrap_err(),
            ProtocolError::BadHeader([0xAB, 0x2E])
        );
    }

    #[test]
    fn rejects_a_bad_trailer() {
        let mut raw = crate::tests::unhex(STATUS);
        raw[42..46].copy_from_slice(&[0, 0, 0, 0]);
        assert_eq!(
            Response::parse(&raw).unwrap_err(),
            ProtocolError::BadTrailer([0, 0, 0, 0])
        );
    }

    #[test]
    fn rejects_a_bad_checksum() {
        let mut raw = crate::tests::unhex(STATUS);
        raw[47] ^= 0x01;
        assert_eq!(
            Response::parse(&raw).unwrap_err(),
            ProtocolError::BadChecksum {
                got: 0x03AA,
                expected: 0x02AA
            }
        );
    }

    #[test]
    fn echo_zero_is_accepted() {
        let response = parse(
            "55 2E 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 \
             00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 48 49 44 43 9B 01",
        );
        response.check_echo(0x15).unwrap();
        response.require_success(0x15).unwrap();
        assert_eq!(response.command_echo(), 0x00);
    }

    #[test]
    fn a_mismatched_echo_is_rejected_but_parses() {
        let response = parse(STATUS);
        let error = response.check_echo(0x11).unwrap_err();
        assert_eq!(
            error,
            ProtocolError::EchoMismatch {
                sent: 0x11,
                received: 0x10
            }
        );
    }

    #[test]
    fn a_device_rejection_is_not_a_malformed_reply() {
        // re/fixtures/05-telemetry-checksum-mismatch.json: valid envelope, result 0x02.
        let response = parse(
            "55 2E 01 02 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 \
             00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 48 49 44 43 9E 01",
        );
        assert_eq!(response.result(), ResultCode::BadChecksum);
        assert_eq!(
            response.require_success(0x01).unwrap_err(),
            ProtocolError::DeviceRejected {
                command: 0x01,
                result: ResultCode::BadChecksum
            }
        );
    }

    #[test]
    fn an_unknown_result_byte_is_preserved() {
        let mut raw = crate::tests::unhex(STATUS);
        raw[3] = 0x7E;
        reseal(&mut raw);
        let response = Response::parse(&raw).unwrap();
        assert_eq!(response.result(), ResultCode::Unknown(0x7E));
    }

    #[test]
    fn identifier_replies_are_redacted() {
        let mut raw = crate::tests::unhex(STATUS);
        raw[2] = 0x12;
        raw[4..36].copy_from_slice(b"0123456789ABCDEF0123456789ABCDEF");
        raw[36..42].fill(0);
        reseal(&mut raw);
        let response = Response::parse(&raw).unwrap();
        let hex = response.redacted_hex();
        assert!(hex.contains(".."), "{hex}");
        assert!(!hex.contains("0123456789ABCDEF"), "{hex}");
        // Leading and trailing framing stays legible.
        assert!(hex.starts_with("55 2E 12 00 .."));
        assert!(hex.contains(".. 00 00 00 00 00 00 48 49 44 43"), "{hex}");
    }

    #[test]
    fn non_identifier_bodies_are_not_masked() {
        // A hypothetical reply whose body is not identifier-shaped must stay readable:
        // the mapper needs those bytes. This is the deliberate limit of the redactor.
        let mut raw = crate::tests::unhex(STATUS);
        raw[2] = 0x03;
        raw[4] = 0x01;
        raw[5] = 0x00;
        reseal(&mut raw);
        let response = Response::parse(&raw).unwrap();
        assert!(response.redacted_hex().contains("55 2E 03 00 01 00"));
        assert!(!response.redacted_hex().contains(".."));
    }
}
