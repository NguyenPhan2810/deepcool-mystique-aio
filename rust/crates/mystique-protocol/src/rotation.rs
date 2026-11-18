//! Rotation, 0..=3.

use std::fmt;

use crate::error::ProtocolError;

/// Panel rotation. Verified: status byte 4 is 0 with the panel upright, and the
/// `0x02` config payload carries it at offset 2. Rotations 1..3 were deliberately
/// never exercised on the unit (`re/findings/03-live-capture.md` §7.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Rotation(u8);

impl Rotation {
    pub const ZERO: Self = Self(0);

    pub fn new(raw: u8) -> Result<Self, ProtocolError> {
        if raw <= 3 {
            Ok(Self(raw))
        } else {
            Err(ProtocolError::RotationOutOfRange(raw))
        }
    }

    pub const fn raw(self) -> u8 {
        self.0
    }

    pub const fn degrees(self) -> u16 {
        self.0 as u16 * 90
    }
}

impl Default for Rotation {
    fn default() -> Self {
        Self::ZERO
    }
}

impl TryFrom<u8> for Rotation {
    type Error = ProtocolError;

    fn try_from(raw: u8) -> Result<Self, Self::Error> {
        Self::new(raw)
    }
}

impl fmt::Display for Rotation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} degrees", self.degrees())
    }
}

impl serde::Serialize for Rotation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_four_positions() {
        for raw in 0u8..=3 {
            let rotation = Rotation::new(raw).unwrap();
            assert_eq!(rotation.raw(), raw);
            assert_eq!(rotation.degrees(), u16::from(raw) * 90);
        }
    }

    #[test]
    fn rejects_five_and_above() {
        assert_eq!(
            Rotation::new(4).unwrap_err(),
            ProtocolError::RotationOutOfRange(4)
        );
        assert!(Rotation::new(0xFF).is_err());
    }
}
