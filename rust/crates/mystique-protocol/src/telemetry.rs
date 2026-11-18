//! Telemetry frame — command `0x01`, 39 payload bytes.
//!
//! # What is proven and what is not
//!
//! The device **accepts without validating** every payload byte: 28 single-variable
//! frames, including out-of-range temperatures and zeroed constants, all returned
//! result `0x00` (`re/findings/03-live-capture.md` §4.3). So the layout below is a
//! *display* contract inherited from the working Windows implementation, not
//! something the firmware enforces.
//!
//! | Field | Basis |
//! |---|---|
//! | offsets 0, 3, 6, 11 (CPU °C, CPU %, MEM %, GPU °C) | inherited; panel is known to show them, never re-verified visually here |
//! | offset 8 (GPU block variant) | **semantics unresolved**; firmware does not band-check it |
//! | offsets 9, 12, 14, 15, 17 (constant bytes) | **unknown**, never varied by production |
//! | offsets 18, 20, 21..22, 24..25 (clock) | high-confidence inference from code and the captured bytes |
//! | offsets 21..22 duplicated at 24..25 | **purpose unresolved**; the device does not require equality |
//! | offsets 1..2, 4..5, 7, 10, 13, 16, 19, 23, 26..38 | **unknown**, zero in every production frame |

use serde::{Deserialize, Serialize};

/// Payload length. Verified: `0x01` with any other length is not produced by the
/// reference implementation, and a 39-byte payload is what the device ACKs.
pub const PAYLOAD_LEN: usize = 39;

/// Known payload offsets. Named so no caller has to remember magic indices.
pub mod offset {
    pub const CPU_TEMPERATURE_C: usize = 0;
    pub const CPU_USAGE_PERCENT: usize = 3;
    pub const MEMORY_LOAD_PERCENT: usize = 6;
    /// GPU block variant / style selector. Semantics unresolved (see module docs).
    pub const GPU_BLOCK_VARIANT: usize = 8;
    /// Constant `0x03` in every production frame. Unknown.
    pub const CONST_9: usize = 9;
    pub const GPU_TEMPERATURE_C: usize = 11;
    /// Constant `0x05`. Unknown.
    pub const CONST_12: usize = 12;
    /// Constant `0x06`. Unknown.
    pub const CONST_14: usize = 14;
    /// Constant `0x0C`. Unknown.
    pub const CONST_15: usize = 15;
    /// Constant `0x07`. Unknown.
    pub const CONST_17: usize = 17;
    /// Whole GHz of the CPU clock.
    pub const CLOCK_GHZ: usize = 18;
    /// Remainder marker, minimum `0x0A`.
    pub const CLOCK_MARKER: usize = 20;
    /// LE16 `((MHz % 1000) / 10) * 100`.
    pub const CLOCK_FRACTION_LE: usize = 21;
    /// A duplicate of [`CLOCK_FRACTION_LE`]; the device does not require equality.
    pub const CLOCK_FRACTION_DUP_LE: usize = 24;
}

/// `0x05`, `0x06`, `0x09`: the three values the production code uses at offset 8,
/// selected by GPU temperature. **Inferred**: acceptance experiments proved the
/// firmware does not cross-check this byte against offset 11, so the banding is a
/// display choice, not a wire rule.
pub const GPU_VARIANT_LOW: u8 = 0x05;
pub const GPU_VARIANT_MID: u8 = 0x06;
pub const GPU_VARIANT_HIGH: u8 = 0x09;

/// Temperature thresholds for the variant above, taken verbatim from
/// `MystiquePackets.BuildDisplayPayload`. **Inferred**; likely an approximation of a
/// vendor curve.
pub const GPU_VARIANT_MID_THRESHOLD_C: u8 = 35;
pub const GPU_VARIANT_HIGH_THRESHOLD_C: u8 = 38;

/// Offset 20 never goes below this in production frames. **Inferred**; the device
/// accepts `0x00` as well.
pub const MIN_CLOCK_MARKER: u8 = 0x0A;

/// The five values the panel is known to display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TelemetryFrame {
    pub cpu_temperature_c: u8,
    pub cpu_usage_percent: u8,
    pub memory_load_percent: u8,
    pub gpu_temperature_c: u8,
    /// Megahertz. Values below 1000 or above 9999 lose information in the
    /// whole-GHz field, exactly as the reference implementation does.
    pub cpu_clock_mhz: u16,
}

impl TelemetryFrame {
    pub const fn new(
        cpu_temperature_c: u8,
        cpu_usage_percent: u8,
        memory_load_percent: u8,
        gpu_temperature_c: u8,
        cpu_clock_mhz: u16,
    ) -> Self {
        Self {
            cpu_temperature_c,
            cpu_usage_percent,
            memory_load_percent,
            gpu_temperature_c,
            cpu_clock_mhz,
        }
    }

    /// Chooses the offset-8 byte from GPU temperature, exactly as the reference
    /// implementation does. Inferred display choice.
    pub const fn gpu_block_variant(&self) -> u8 {
        if self.gpu_temperature_c < GPU_VARIANT_MID_THRESHOLD_C {
            GPU_VARIANT_LOW
        } else if self.gpu_temperature_c < GPU_VARIANT_HIGH_THRESHOLD_C {
            GPU_VARIANT_MID
        } else {
            GPU_VARIANT_HIGH
        }
    }

    /// The 39-byte payload. The remaining bytes stay zero, matching every captured
    /// production frame.
    pub fn encode(&self) -> [u8; PAYLOAD_LEN] {
        let mut payload = [0u8; PAYLOAD_LEN];

        payload[offset::CPU_TEMPERATURE_C] = self.cpu_temperature_c;
        payload[offset::CPU_USAGE_PERCENT] = self.cpu_usage_percent;
        payload[offset::MEMORY_LOAD_PERCENT] = self.memory_load_percent;
        payload[offset::GPU_BLOCK_VARIANT] = self.gpu_block_variant();
        payload[offset::CONST_9] = 0x03;
        payload[offset::GPU_TEMPERATURE_C] = self.gpu_temperature_c;
        payload[offset::CONST_12] = 0x05;
        payload[offset::CONST_14] = 0x06;
        payload[offset::CONST_15] = 0x0C;
        payload[offset::CONST_17] = 0x07;

        let whole_ghz = (self.cpu_clock_mhz / 1000) as u8;
        let remainder = self.cpu_clock_mhz % 1000;
        let marker = (remainder / 10) as u8;
        let fraction = u16::from(marker) * 100;

        payload[offset::CLOCK_GHZ] = whole_ghz;
        payload[offset::CLOCK_MARKER] = marker.max(MIN_CLOCK_MARKER);
        payload[offset::CLOCK_FRACTION_LE..offset::CLOCK_FRACTION_LE + 2]
            .copy_from_slice(&fraction.to_le_bytes());
        payload[offset::CLOCK_FRACTION_DUP_LE..offset::CLOCK_FRACTION_DUP_LE + 2]
            .copy_from_slice(&fraction.to_le_bytes());

        payload
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact payload of `re/fixtures/04-telemetry-baseline-0x01.json`:
    /// CPU 36 °C, 9 %, 39 % RAM, GPU 48 °C, 4300 MHz.
    const BASELINE: [u8; 39] = [
        0x24, 0x00, 0x00, 0x09, 0x00, 0x00, 0x27, 0x00, 0x09, 0x03, 0x00, 0x30, 0x05, 0x00, 0x06,
        0x0C, 0x00, 0x07, 0x04, 0x00, 0x1E, 0xB8, 0x0B, 0x00, 0xB8, 0x0B, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn matches_the_captured_baseline_frame() {
        let frame = TelemetryFrame::new(36, 9, 39, 48, 4300);
        assert_eq!(frame.encode(), BASELINE);
    }

    #[test]
    fn clock_fields_follow_the_documented_arithmetic() {
        let frame = TelemetryFrame::new(0, 0, 0, 0, 4300);
        let payload = frame.encode();
        assert_eq!(payload[offset::CLOCK_GHZ], 4);
        assert_eq!(payload[offset::CLOCK_MARKER], 30);
        assert_eq!(
            u16::from_le_bytes([payload[21], payload[22]]),
            3000,
            "((4300 % 1000) / 10) * 100"
        );
        assert_eq!(&payload[21..23], &payload[24..26]);
    }

    #[test]
    fn clock_below_one_ghz_keeps_the_marker_floor() {
        // The reference implementation clamps the marker but computes the fraction
        // from the unclamped remainder.
        let payload = TelemetryFrame::new(0, 0, 0, 0, 50).encode();
        assert_eq!(payload[offset::CLOCK_GHZ], 0);
        assert_eq!(payload[offset::CLOCK_MARKER], MIN_CLOCK_MARKER);
        assert_eq!(u16::from_le_bytes([payload[21], payload[22]]), 500);

        // 800 MHz: raw marker 80, above the floor, so both fields are untouched.
        let payload = TelemetryFrame::new(0, 0, 0, 0, 800).encode();
        assert_eq!(payload[offset::CLOCK_MARKER], 80);
        assert_eq!(u16::from_le_bytes([payload[21], payload[22]]), 8000);
    }

    #[test]
    fn gpu_variant_bands_match_the_reference_implementation() {
        assert_eq!(
            TelemetryFrame::new(0, 0, 0, 34, 0).gpu_block_variant(),
            0x05
        );
        assert_eq!(
            TelemetryFrame::new(0, 0, 0, 35, 0).gpu_block_variant(),
            0x06
        );
        assert_eq!(
            TelemetryFrame::new(0, 0, 0, 37, 0).gpu_block_variant(),
            0x06
        );
        assert_eq!(
            TelemetryFrame::new(0, 0, 0, 38, 0).gpu_block_variant(),
            0x09
        );
        assert_eq!(
            TelemetryFrame::new(0, 0, 0, 90, 0).gpu_block_variant(),
            0x09
        );
    }

    #[test]
    fn unknown_offsets_stay_zero() {
        let payload = TelemetryFrame::new(1, 2, 3, 4, 5).encode();
        for index in [1, 2, 4, 5, 7, 10, 13, 16, 19, 23] {
            assert_eq!(payload[index], 0, "offset {index}");
        }
        assert!(payload[26..].iter().all(|byte| *byte == 0));
    }
}
