//! The panel's clock: one wall-clock reading, as `0x0A` carries it.
//!
//! `0x0A` is not a mode switch and its last byte is not a counter
//! (`re/findings/22-clock.md`). The payload is
//! `EA 07 <month> <day> <hour> <minute> <second>`, and the vendor sends it about once
//! a second. [`Clock`] keeps the five values together so the init sequence and the
//! upload preamble cannot be handed a frozen date or a sequence number by accident.
//!
//! Callers with no clock source — tests, fixtures, dry runs — use [`Clock::zero`], which
//! is an explicit "unknown", not a real date that would look plausible on the panel.

use crate::commands::{time_payload, TIME_PAYLOAD_LEN};

/// One local wall-clock reading, in the encoding `0x0A` expects.
///
/// The values are local time, not UTC: the vendor app sends local time and the panel
/// draws it verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Clock {
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl Clock {
    /// The explicit "no clock source" reading: all five fields zero.
    pub const ZERO: Self = Self {
        month: 0,
        day: 0,
        hour: 0,
        minute: 0,
        second: 0,
    };

    pub const fn new(month: u8, day: u8, hour: u8, minute: u8, second: u8) -> Self {
        Self {
            month,
            day,
            hour,
            minute,
            second,
        }
    }

    /// The explicit "no clock source" reading. Never a frozen real date.
    pub const fn zero() -> Self {
        Self::ZERO
    }

    /// The seven `0x0A` payload bytes.
    pub const fn payload(self) -> [u8; TIME_PAYLOAD_LEN] {
        time_payload(self.month, self.day, self.hour, self.minute, self.second)
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::ZERO
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::TIME_HEADER;

    #[test]
    fn zero_is_an_explicit_empty_clock_not_a_plausible_date() {
        assert_eq!(Clock::zero(), Clock::ZERO);
        assert_eq!(Clock::default(), Clock::zero());
        assert_eq!(Clock::zero().payload(), [0xEA, 0x07, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn each_field_lands_at_its_own_offset_behind_the_header() {
        let clock = Clock::new(1, 2, 3, 4, 5);
        let payload = clock.payload();
        assert_eq!(payload.len(), TIME_PAYLOAD_LEN);
        assert_eq!(&payload[..2], &TIME_HEADER);
        assert_eq!(
            payload[2..],
            [
                clock.month,
                clock.day,
                clock.hour,
                clock.minute,
                clock.second
            ]
        );
    }
}
