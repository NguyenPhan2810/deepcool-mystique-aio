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
//! | offset 18 (whole GHz) and 20 (fraction in 10 MHz) | verified against the vendor app, 2026-11-19 |
//! | offsets 21..22 (CPU fan RPM) and 24..25 (pump RPM) | **verified, 2026-11-19**: the panel's two dials draw these slots, and the vendor frames carried the bridge's `Fan list` (`CPU`, `AIO Pump`) in them |
//! | offsets 1..2, 4..5, 7, 10, 13, 16, 19, 23, 26..38 | **unknown**, zero in every captured vendor frame |
//!
//! # Correction, 2026-11-19
//!
//! This crate used to write the CPU clock fraction into **both** 21..22 and 24..25,
//! believing them a duplicated clock field. They are the two RPM dials — **21..22 is CPU
//! FAN, 24..25 is PUMP RPM** — and the panel dutifully drew the clock fraction as fan and
//! pump speed, which is why both dials showed the same number in the 1000–8000 range and
//! moved with the CPU clock.
//!
//! The first correction labelled the slots the other way round, misled by a capture taken
//! while the owner's two Fan I/O mappings were crossed: the vendor app was feeding the
//! pump reading into the CPU slot, so the steady 3341 appeared at 21..22. The panel's own
//! dial order is the authority, and it put CPU first.
//!
//! The clock is fully described by offset 18 and 20 alone.

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
    /// Remainder in 10 MHz units, `(MHz % 1000) / 10`, floor `0x0A` in production.
    pub const CLOCK_MARKER: usize = 20;
    /// LE16 CPU fan speed in RPM. Verified on the panel, 2026-11-19: the first dial of
    /// `CPU FanSpeed + Pump Speed` draws this slot.
    pub const CPU_FAN_RPM_LE: usize = 21;
    /// LE16 pump speed in RPM. Verified on the panel, 2026-11-19: the second dial draws
    /// this slot.
    pub const PUMP_RPM_LE: usize = 24;
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

/// The values the panel is known to display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TelemetryFrame {
    pub cpu_temperature_c: u8,
    pub cpu_usage_percent: u8,
    pub memory_load_percent: u8,
    pub gpu_temperature_c: u8,
    /// Megahertz. Values below 1000 or above 9999 lose information in the
    /// whole-GHz field, exactly as the reference implementation does.
    pub cpu_clock_mhz: u16,
    /// `0x01[21..22]`. `0` means "no reading", which is what the panel draws as 0 RPM.
    #[serde(default)]
    pub pump_rpm: u16,
    /// `0x01[24..25]`. `0` means "no reading", which is what the panel draws as 0 RPM.
    #[serde(default)]
    pub cpu_fan_rpm: u16,
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
            pump_rpm: 0,
            cpu_fan_rpm: 0,
        }
    }

    /// Adds the two RPM readings. Kept separate from [`new`] so the five-value frame
    /// stays the baseline every capture and test was written against.
    pub const fn with_fans(mut self, pump_rpm: u16, cpu_fan_rpm: u16) -> Self {
        self.pump_rpm = pump_rpm;
        self.cpu_fan_rpm = cpu_fan_rpm;
        self
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

        payload[offset::CLOCK_GHZ] = (self.cpu_clock_mhz / 1000) as u8;
        let marker = ((self.cpu_clock_mhz % 1000) / 10) as u8;
        payload[offset::CLOCK_MARKER] = marker.max(MIN_CLOCK_MARKER);

        payload[offset::CPU_FAN_RPM_LE..offset::CPU_FAN_RPM_LE + 2]
            .copy_from_slice(&self.cpu_fan_rpm.to_le_bytes());
        payload[offset::PUMP_RPM_LE..offset::PUMP_RPM_LE + 2]
            .copy_from_slice(&self.pump_rpm.to_le_bytes());

        payload
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact payload `MystiquePackets.BuildDisplayPayload` emitted for these values
    /// before the 2026-11-19 correction, except that the two RPM slots are now zero:
    /// that encoder wrote the clock fraction there, which is the bug this crate fixed.
    const BASELINE: [u8; 39] = [
        0x24, 0x00, 0x00, 0x09, 0x00, 0x00, 0x27, 0x00, 0x09, 0x03, 0x00, 0x30, 0x05, 0x00, 0x06,
        0x0C, 0x00, 0x07, 0x04, 0x00, 0x1E, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
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
        // The clock is fully described by 18 and 20; the RPM slots are not its fraction.
        assert_eq!(u16::from_le_bytes([payload[21], payload[22]]), 0);
        assert_eq!(u16::from_le_bytes([payload[24], payload[25]]), 0);
    }

    #[test]
    fn fans_use_their_own_little_endian_slots() {
        // The vendor frame captured 2026-11-19 carried the sensor bridge's own readings:
        // `Fan list: { "AIO Pump": 3341, "CPU": 1326 }`. The panel's first dial draws
        // 21..22 (CPU FAN) and its second draws 24..25 (PUMP RPM) — confirmed by the
        // owner seeing them crossed when the two slots were.
        let payload = TelemetryFrame::new(36, 9, 39, 48, 4300)
            .with_fans(3341, 1326)
            .encode();
        assert_eq!(
            u16::from_le_bytes([payload[21], payload[22]]),
            1326,
            "CPU fan RPM at 21..22"
        );
        assert_eq!(
            u16::from_le_bytes([payload[24], payload[25]]),
            3341,
            "pump RPM at 24..25"
        );
        // A fan reading must never disturb the clock fields.
        assert_eq!(payload[offset::CLOCK_GHZ], 4);
        assert_eq!(payload[offset::CLOCK_MARKER], 30);
    }

    #[test]
    fn clock_below_one_ghz_keeps_the_marker_floor() {
        let payload = TelemetryFrame::new(0, 0, 0, 0, 50).encode();
        assert_eq!(payload[offset::CLOCK_GHZ], 0);
        assert_eq!(payload[offset::CLOCK_MARKER], MIN_CLOCK_MARKER);

        // 800 MHz: raw marker 80, above the floor, so it is written untouched.
        let payload = TelemetryFrame::new(0, 0, 0, 0, 800).encode();
        assert_eq!(payload[offset::CLOCK_MARKER], 80);
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
