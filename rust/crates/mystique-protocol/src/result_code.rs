use std::fmt;

use serde::{Serialize, Serializer};

/// The device's verdict on a request, carried in response byte 3.
///
/// Verified on the real device:
///
/// | Raw | Meaning |
/// |---|---|
/// | `0x00` | accepted |
/// | `0x01` | the `HIDC` trailer marker was wrong |
/// | `0x02` | the request checksum was wrong — **the command still executes** |
///
/// `0x02` is not "the request was ignored": a `0x10` probe with a broken checksum
/// still returns rotation and mode (`re/fixtures/08-status-checksum-mismatch.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResultCode {
    Ok,
    BadMarker,
    BadChecksum,
    /// Any byte the device sends that is not one of the three documented codes.
    Unknown(u8),
}

impl ResultCode {
    pub const fn from_raw(raw: u8) -> Self {
        match raw {
            0x00 => Self::Ok,
            0x01 => Self::BadMarker,
            0x02 => Self::BadChecksum,
            other => Self::Unknown(other),
        }
    }

    pub const fn as_raw(self) -> u8 {
        match self {
            Self::Ok => 0x00,
            Self::BadMarker => 0x01,
            Self::BadChecksum => 0x02,
            Self::Unknown(raw) => raw,
        }
    }

    pub const fn is_ok(self) -> bool {
        matches!(self, Self::Ok)
    }
}

impl fmt::Display for ResultCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok => f.write_str("success"),
            Self::BadMarker => f.write_str("bad trailer marker (0x01)"),
            Self::BadChecksum => f.write_str("bad checksum (0x02)"),
            Self::Unknown(raw) => write!(f, "unknown result 0x{raw:02X}"),
        }
    }
}

impl Serialize for ResultCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let text = match self {
            Self::Ok => "ok".to_owned(),
            Self::BadMarker => "bad_marker".to_owned(),
            Self::BadChecksum => "bad_checksum".to_owned(),
            Self::Unknown(raw) => format!("unknown:0x{raw:02X}"),
        };
        serializer.serialize_str(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_byte() {
        for raw in 0u8..=0xFF {
            assert_eq!(ResultCode::from_raw(raw).as_raw(), raw);
        }
    }

    #[test]
    fn names_the_three_documented_codes() {
        assert_eq!(ResultCode::from_raw(0x00), ResultCode::Ok);
        assert_eq!(ResultCode::from_raw(0x01), ResultCode::BadMarker);
        assert_eq!(ResultCode::from_raw(0x02), ResultCode::BadChecksum);
        assert_eq!(ResultCode::from_raw(0x7F), ResultCode::Unknown(0x7F));
        assert!(ResultCode::Ok.is_ok());
        assert!(!ResultCode::BadChecksum.is_ok());
    }

    #[test]
    fn serializes_as_a_stable_string() {
        let json = serde_json::to_string(&ResultCode::BadChecksum).unwrap();
        assert_eq!(json, "\"bad_checksum\"");
        let json = serde_json::to_string(&ResultCode::Unknown(0x7F)).unwrap();
        assert_eq!(json, "\"unknown:0x7F\"");
    }
}
