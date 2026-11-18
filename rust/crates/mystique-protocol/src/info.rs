//! Device identity — command `0x12`.

use std::fmt;

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use crate::commands::CMD_DEVICE_INFO;
use crate::error::ProtocolError;
use crate::response::Response;

pub const IDENTIFIER_OFFSET: usize = 4;
pub const IDENTIFIER_LEN: usize = 32;
pub const IDENTIFIER_END: usize = IDENTIFIER_OFFSET + IDENTIFIER_LEN;

/// The decoded `0x12` reply.
///
/// Verified (`re/fixtures/01-info-0x12.json`): 48 bytes, echo `0x12`, result `0x00`,
/// 32 uppercase hex characters at 4..35, then six zero bytes.
///
/// The identifier is per-device and is **redacted by default everywhere**: `Debug`,
/// `Display` and `Serialize` all print `<redacted>`. Reading it requires an explicit
/// [`DeviceInfo::reveal_identifier`] call, which exists for interactive diagnostics
/// only and must never be used in logs or reports.
#[derive(Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    identifier: [u8; IDENTIFIER_LEN],
    padding_is_zero: bool,
}

impl DeviceInfo {
    /// Validates the envelope, the echo and the result, then extracts the identifier.
    ///
    /// The identifier alphabet is *not* enforced: `identifier_is_hex` reports it
    /// instead. Refusing an unexpected alphabet would make the whole init handshake
    /// fail on a future hardware revision for no protocol reason.
    pub fn parse(response: &Response) -> Result<Self, ProtocolError> {
        if response.wire_len() < 48 {
            return Err(ProtocolError::InfoLength(response.wire_len()));
        }
        response.require_success(CMD_DEVICE_INFO)?;

        let mut identifier = [0u8; IDENTIFIER_LEN];
        identifier.copy_from_slice(&response.as_bytes()[IDENTIFIER_OFFSET..IDENTIFIER_END]);

        Ok(Self {
            identifier,
            padding_is_zero: response.as_bytes()[IDENTIFIER_END..42]
                .iter()
                .all(|byte| *byte == 0),
        })
    }

    /// True when all 32 bytes are uppercase hex, which is what this unit sends.
    pub fn identifier_is_hex(&self) -> bool {
        self.identifier
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(byte))
    }

    /// True when bytes 36..42 were zero, as observed.
    pub const fn padding_is_zero(&self) -> bool {
        self.padding_is_zero
    }

    /// The identifier as text. **Deliberately explicit.**
    ///
    /// Using this in a log, a report, or a commit leaks a per-device serial. The only
    /// intended caller is a human running `mystique info --show-device-id`.
    pub fn reveal_identifier(&self) -> String {
        String::from_utf8_lossy(&self.identifier).into_owned()
    }

    /// A non-reversible per-device tag for correlating reports, e.g. `id:3f2a`.
    /// Derived from the identifier without printing it.
    pub fn fingerprint(&self) -> String {
        let sum = self.identifier.iter().fold(0u32, |acc, byte| {
            acc.wrapping_mul(31).wrapping_add(u32::from(*byte))
        });
        format!("id:{:04x}", sum & 0xFFFF)
    }
}

impl fmt::Debug for DeviceInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceInfo")
            .field("identifier", &"<redacted>")
            .field("identifier_is_hex", &self.identifier_is_hex())
            .field("padding_is_zero", &self.padding_is_zero)
            .finish()
    }
}

impl fmt::Display for DeviceInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "device <redacted> ({} hex identifier, {} zero padding)",
            if self.identifier_is_hex() {
                "valid"
            } else {
                "non-hex"
            },
            if self.padding_is_zero {
                "with"
            } else {
                "without"
            }
        )
    }
}

impl Serialize for DeviceInfo {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("DeviceInfo", 3)?;
        state.serialize_field("identifier", "<redacted>")?;
        state.serialize_field("identifier_is_hex", &self.identifier_is_hex())?;
        state.serialize_field("padding_is_zero", &self.padding_is_zero)?;
        state.end()
    }
}

/// True when `body` (response bytes 4..42) matches the identifier shape:
/// 32 uppercase-hex bytes followed by six zeros.
///
/// Used by [`Response::redacted_hex`](crate::Response::redacted_hex) so that unknown
/// command bodies stay readable unless they could be an identifier.
pub fn looks_like_identifier(body: &[u8]) -> bool {
    if body.len() != 38 {
        return false;
    }
    let hex = &body[..IDENTIFIER_LEN];
    let padding = &body[IDENTIFIER_LEN..];
    hex.iter()
        .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(byte))
        && padding.iter().all(|byte| *byte == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::checksum16;

    fn info_reply(identifier: &[u8; 32]) -> Response {
        let mut raw = [0u8; 48];
        raw[0] = 0x55;
        raw[1] = 0x2E;
        raw[2] = 0x12;
        raw[3] = 0x00;
        raw[4..36].copy_from_slice(identifier);
        raw[42..46].copy_from_slice(b"HIDC");
        let sum = checksum16(&raw[..46]);
        raw[46..48].copy_from_slice(&sum.to_le_bytes());
        Response::parse(&raw).unwrap()
    }

    #[test]
    fn parses_the_identifier_shape() {
        let response = info_reply(b"0123456789ABCDEF0123456789ABCDEF");
        let info = DeviceInfo::parse(&response).unwrap();
        assert!(info.identifier_is_hex());
        assert!(info.padding_is_zero());
        assert_eq!(info.reveal_identifier(), "0123456789ABCDEF0123456789ABCDEF");
    }

    #[test]
    fn debug_display_and_serde_are_redacted() {
        let response = info_reply(b"DEADBEEFDEADBEEFDEADBEEFDEADBEEF");
        let info = DeviceInfo::parse(&response).unwrap();

        assert!(!format!("{info:?}").contains("DEADBEEF"));
        assert!(!format!("{info}").contains("DEADBEEF"));
        assert!(format!("{info:?}").contains("<redacted>"));

        let json = serde_json::to_string(&info).unwrap();
        assert!(!json.contains("DEADBEEF"), "{json}");
        assert!(json.contains("<redacted>"), "{json}");
    }

    #[test]
    fn rejects_a_non_success_result() {
        let mut raw = [0u8; 48];
        raw[0] = 0x55;
        raw[1] = 0x2E;
        raw[2] = 0x12;
        raw[3] = 0x02;
        raw[42..46].copy_from_slice(b"HIDC");
        let sum = checksum16(&raw[..46]);
        raw[46..48].copy_from_slice(&sum.to_le_bytes());
        let response = Response::parse(&raw).unwrap();
        assert!(DeviceInfo::parse(&response).is_err());
    }

    #[test]
    fn detects_the_identifier_shape() {
        let mut body = [0u8; 38];
        body[..32].copy_from_slice(b"0123456789ABCDEF0123456789ABCDEF");
        assert!(looks_like_identifier(&body));

        body[0] = b'z';
        assert!(!looks_like_identifier(&body));

        body[0] = b'0';
        body[37] = 1;
        assert!(!looks_like_identifier(&body));

        assert!(!looks_like_identifier(&[0u8; 41]));
    }

    #[test]
    fn fingerprint_is_stable_and_does_not_leak() {
        let response = info_reply(b"0123456789ABCDEF0123456789ABCDEF");
        let info = DeviceInfo::parse(&response).unwrap();
        assert_eq!(info.fingerprint(), info.fingerprint());
        assert!(info.fingerprint().starts_with("id:"));
        assert!(!info.fingerprint().contains("0123456789ABCDEF"));
    }
}
