//! Typed display settings: `0x02`, `0x04`, `0x05` and `0x07`.
//!
//! Everything here comes from the 2026-11-18 captures in which the owner changed one
//! control at a time in the vendor app: `re/findings/13-settings-decode.md`,
//! `re/findings/16-aux-area-layout.md`, `re/findings/17-mode-and-idle-screen.md` and
//! `re/findings/18-rgb-and-recording.md`. That method makes a field unambiguous when it
//! is the only byte that moved in a block; the confidence of each field is repeated on
//! the type that carries it.
//!
//! # One payload, no partial writes
//!
//! `0x02` is always five meaningful bytes: idle screen, gyro, rotation, RGB sync and
//! brightness. The device treats the payload as absolute state, not as a delta.
//! [`DisplayConfig`] is the only encoder: every typed operation builds a complete
//! payload through [`DisplayConfig::payload`], and there is no way to send "just
//! brightness" as a one-byte `0x02`.
//!
//! The consequence is honest and important: a standalone `set_brightness` also sends
//! the other four fields at their defaults. A caller that must preserve them keeps its
//! own [`DisplayConfig`]; the daemon does exactly that for the whole session.
//!
//! # Idle screen is not screen power
//!
//! `0x02[0]` is the idle screen display toggle. It is not screen enable and it does not
//! blank the panel; brightness `0` does (`re/findings/15-driver-verification.md`).
//!
//! # The one device behaviour that matters
//!
//! **Rotation is ignored while the gyroscopes are enabled.** The vendor UI enforces
//! this by disabling the rotation control until the gyro is off
//! (`re/findings/13-settings-decode.md`, "Behavioural rule discovered by the owner").
//! [`rotate_with_gyro_off`] exists so that rule is named at the call site instead of
//! being rediscovered by whoever wonders why a rotation command "did nothing".
//!
//! # `0x03` has no encoder here
//!
//! `0x03` selects the display mode (`01` Monitor, `02` Media, `03` Recording,
//! `re/findings/17-mode-and-idle-screen.md`), but stored content dominates the panel
//! while it exists. The driver does not send mode writes yet; `0x14` erase is the
//! verified way back from stored media (`re/findings/15-driver-verification.md`).

mod config;
mod recording;

pub use config::{
    rotate_with_gyro_off, set_brightness, set_gyro, set_idle_screen, set_rgb_sync, set_rotation,
    DisplayConfig, BRIGHTNESS_MAX,
};
pub use recording::{set_recording, RecordingConfig, RECORDING_OPTIONS_LEN};

use crate::commands::Command;
use crate::error::ProtocolError;
use crate::init::SETTLE_STEP;
use crate::upload::CommandPacket;

/// Payload length of `0x04`: main at index 0, style at index 1, index 2 reserved,
/// auxiliary at index 3.
///
/// Four bytes is the layout the vendor app sends (`re/findings/16-aux-area-layout.md`).
/// An earlier two-byte reading put the auxiliary selector at index 1, where the panel
/// ignores it; index 1 is Style Type.
pub const DISPLAY_AREAS_LEN: usize = 4;

/// Payload length of `0x07`: switch time at index 0, animation at index 1.
pub const MEDIA_TIMING_LEN: usize = 2;

/// Highest display-area selector the driver will send.
///
/// The capture proves `00`, `01`, `03` and `05` for the main area, and `00`, `01` and
/// `02` for the auxiliary area (`re/findings/16-aux-area-layout.md`). That is still not
/// a closed option list, so the driver accepts any nibble and rejects only values whose
/// high bits cannot be a selector. Inventing an enum here would make the driver reject
/// options the firmware may support.
pub const DISPLAY_AREA_MAX: u8 = 0x0F;

/// Style Type values at `0x04` index 1 (`re/findings/16-aux-area-layout.md`).
///
/// The capture steps through exactly these three values on the primary area; the
/// selector only renders when main is `00` CPU Frequency.
pub const STYLE_NORMAL: u8 = 0x00;
pub const STYLE_RING: u8 = 0x01;
pub const STYLE_STOPWATCH: u8 = 0x02;

/// Highest Style Type value the driver will send. Unlike the main and auxiliary
/// selectors, the capture closes this list at three values.
pub const DISPLAY_STYLE_MAX: u8 = STYLE_STOPWATCH;

/// Highest media timing selector. The capture shows exactly `00`, `01` and `02` for
/// both the switch time and the animation (`re/findings/13-settings-decode.md` § `0x07`).
pub const MEDIA_TIMING_MAX: u8 = 0x02;

// ---------------------------------------------------------------------------
// 0x04 — display areas
// ---------------------------------------------------------------------------

/// `0x04`: the main and auxiliary display-area selectors plus the primary Style Type,
/// packed into one four-byte payload.
///
/// | Index | Meaning | Values | Confidence |
/// |---|---|---|---|
/// | 0 | Main Display Area | observed `00`, `01`, `03`, `05` | High |
/// | 1 | Style Type | `00` Normal, `01` Ring, `02` Stopwatch | Verified |
/// | 2 | — | always written `00`; never observed non-zero | **Unknown** |
/// | 3 | Auxiliary style | `00` Voltage Mode, `01` System Monitor, `02` Core Data | Verified |
///
/// The layout is from the 2026-11-18 USBPcap captures
/// (`re/findings/16-aux-area-layout.md`): in the style capture byte 1 moved
/// `00 → 01 → 02` (`00 00 00 02` → `00 01 00 02` → `00 02 00 02`), and in the
/// auxiliary capture byte 3 moved `02 → 00 → 01 → 02` while bytes 1 and 2 stayed zero.
/// The original C# implementation sends `05 00 00 01` — main Temperature, Style Type
/// Normal, auxiliary System Monitor — which is the same four-byte layout.
///
/// Index 2 has never been observed non-zero, so its meaning stays **Unknown**; the
/// driver always writes `00`. Byte 1 was previously mistaken for the auxiliary selector,
/// which is why the payload used to be modelled as two bytes.
///
/// Style Type is a **presentation style** for the primary field: `00` Normal, `01` Ring,
/// `02` Stopwatch. Index 3 is the same for the secondary field: `00` Voltage Mode draws
/// CPU frequency plus voltage, `01` System Monitor adds a ring, `02` Core Data draws a
/// speedometer. Both only have a visible effect when index 0 is `00` (CPU Frequency);
/// the other five main layouts ignore them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayAreas {
    main: u8,
    style: u8,
    aux: u8,
}

impl Default for DisplayAreas {
    /// Main `00` (CPU Frequency), Style Type `00` (Normal) and auxiliary `00` (Voltage
    /// Mode), the values the captured init block carries.
    fn default() -> Self {
        Self {
            main: 0,
            style: STYLE_NORMAL,
            aux: 0,
        }
    }
}

impl DisplayAreas {
    /// Main and auxiliary accept any nibble; Style Type is the closed `0..=2` list.
    /// There is no closed option list for the other two in the findings, so nothing
    /// narrower is enforced there.
    pub fn new(main: u8, style: u8, aux: u8) -> Result<Self, ProtocolError> {
        for value in [main, aux] {
            if value > DISPLAY_AREA_MAX {
                return Err(ProtocolError::DisplayAreaOutOfRange(value));
            }
        }
        if style > DISPLAY_STYLE_MAX {
            return Err(ProtocolError::DisplayStyleOutOfRange(style));
        }
        Ok(Self { main, style, aux })
    }

    pub const fn main(&self) -> u8 {
        self.main
    }

    pub const fn style(&self) -> u8 {
        self.style
    }

    pub const fn aux(&self) -> u8 {
        self.aux
    }

    /// Four bytes, `[main, style, 0, aux]`; index 2 is always zero.
    pub const fn payload(&self) -> [u8; DISPLAY_AREAS_LEN] {
        [self.main, self.style, 0, self.aux]
    }

    pub fn packet(&self) -> CommandPacket {
        CommandPacket::new(Command::InitStep04, self.payload(), SETTLE_STEP)
    }
}

/// Sets all three display-area selectors in `0x04`. `Command::InitStep04` is the `0x04`
/// variant; its name predates the settings decode.
pub fn set_display_areas(main: u8, style: u8, aux: u8) -> Result<CommandPacket, ProtocolError> {
    Ok(DisplayAreas::new(main, style, aux)?.packet())
}

// ---------------------------------------------------------------------------
// 0x07 — media switch timing
// ---------------------------------------------------------------------------

/// `0x07`: image switch time and switch animation.
///
/// Both bytes have exactly three observed values, and the sequence
/// `00 01` → `01 01` → `02 01` → `02 02` → `00 00` → `00 01` follows the owner
/// stepping through the two controls. Confidence: **high**.
///
/// | Byte 0 (time) | Duration | Byte 1 (animation) | Effect |
/// |---|---|---|---|
/// | `00` | 3 s | `00` | Split Display |
/// | `01` | 5 s | `01` | Scroll |
/// | `02` | 7 s | `02` | Fade In/Out |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaTiming {
    time: u8,
    animation: u8,
}

impl MediaTiming {
    pub fn new(time: u8, animation: u8) -> Result<Self, ProtocolError> {
        for value in [time, animation] {
            if value > MEDIA_TIMING_MAX {
                return Err(ProtocolError::MediaTimingOutOfRange(value));
            }
        }
        Ok(Self { time, animation })
    }

    pub const fn time(&self) -> u8 {
        self.time
    }

    pub const fn animation(&self) -> u8 {
        self.animation
    }

    /// The switch interval the firmware will use: 3, 5 or 7 seconds.
    pub const fn switch_seconds(&self) -> u8 {
        3 + 2 * self.time
    }

    /// The animation name as the vendor UI spells it.
    pub const fn animation_name(&self) -> &'static str {
        match self.animation {
            0 => "Split Display",
            1 => "Scroll",
            _ => "Fade In/Out",
        }
    }

    pub const fn payload(&self) -> [u8; MEDIA_TIMING_LEN] {
        [self.time, self.animation]
    }

    pub fn packet(&self) -> CommandPacket {
        CommandPacket::new(Command::InitStep07, self.payload(), SETTLE_STEP)
    }
}

/// Sets the image switch time and animation in `0x07`. `Command::InitStep07` is the
/// `0x07` variant; its name predates the settings decode.
pub fn set_media_timing(time: u8, animation: u8) -> Result<CommandPacket, ProtocolError> {
    Ok(MediaTiming::new(time, animation)?.packet())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::Pipe;

    #[test]
    fn display_areas_accept_the_observed_values_and_reject_out_of_range() {
        for (main, style, aux) in [(0, 0, 0), (1, 0, 0), (3, 1, 2), (5, 0, 2), (0, 2, 1)] {
            let areas = DisplayAreas::new(main, style, aux).unwrap();
            assert_eq!(areas.main(), main);
            assert_eq!(areas.style(), style);
            assert_eq!(areas.aux(), aux);
            assert_eq!(areas.payload(), [main, style, 0, aux]);
        }
        assert_eq!(
            DisplayAreas::new(0x10, 0, 0).unwrap_err(),
            ProtocolError::DisplayAreaOutOfRange(0x10)
        );
        assert_eq!(
            DisplayAreas::new(0, 0, 0xFF).unwrap_err(),
            ProtocolError::DisplayAreaOutOfRange(0xFF)
        );
        assert_eq!(
            DisplayAreas::new(0, 3, 0).unwrap_err(),
            ProtocolError::DisplayStyleOutOfRange(3)
        );
        assert_eq!(
            DisplayAreas::new(0, 0xFF, 0).unwrap_err(),
            ProtocolError::DisplayStyleOutOfRange(0xFF)
        );
        assert_eq!(
            set_display_areas(5, 1, 2).unwrap().payload,
            vec![5, 1, 0, 2]
        );
    }

    #[test]
    fn display_areas_payload_is_main_style_zero_aux() {
        // Byte for byte what the original C# implementation sends for
        // main Temperature (5), Style Type Normal (0), auxiliary System Monitor (1).
        let areas = DisplayAreas::new(5, STYLE_NORMAL, 1).unwrap();
        assert_eq!(DISPLAY_AREAS_LEN, 4);
        assert_eq!(areas.payload(), [0x05, 0x00, 0x00, 0x01]);

        // Aux only ever moves index 3; the reserved byte stays zero.
        let aux = DisplayAreas::new(0, STYLE_NORMAL, 2).unwrap().payload();
        assert_eq!(aux, [0x00, 0x00, 0x00, 0x02]);
        assert_eq!(aux[1], STYLE_NORMAL);
        assert_eq!(aux[2], 0x00);

        assert_eq!(
            set_display_areas(5, STYLE_RING, 1).unwrap().payload,
            vec![0x05, 0x01, 0x00, 0x01]
        );
        assert_eq!(DisplayAreas::default().style(), STYLE_NORMAL);
    }

    #[test]
    fn style_capture_transition_is_pinned() {
        // re/findings/16-aux-area-layout.md: 00 00 00 02 -> 00 01 00 02 -> 00 02 00 02
        // as the owner stepped Normal -> Ring -> Stopwatch with main CPU Frequency (0)
        // and auxiliary Core Data (2).
        let normal = DisplayAreas::new(0, STYLE_NORMAL, 2).unwrap();
        let ring = DisplayAreas::new(0, STYLE_RING, 2).unwrap();
        let stopwatch = DisplayAreas::new(0, STYLE_STOPWATCH, 2).unwrap();
        assert_eq!(normal.payload(), [0x00, 0x00, 0x00, 0x02]);
        assert_eq!(ring.payload(), [0x00, 0x01, 0x00, 0x02]);
        assert_eq!(stopwatch.payload(), [0x00, 0x02, 0x00, 0x02]);
    }

    #[test]
    fn display_areas_route_to_0x04_on_the_init_pipe() {
        let packet = set_display_areas(3, STYLE_RING, 2).unwrap();
        assert_eq!(packet.command, Command::InitStep04);
        assert_eq!(packet.code(), 0x04);
        assert_eq!(packet.pipe(), Pipe::Init);
        assert_eq!(packet.packet().unwrap()[2], 0x04);
    }

    #[test]
    fn media_timing_accepts_exactly_three_values_in_each_byte() {
        for time in 0u8..=2 {
            for animation in 0u8..=2 {
                let timing = MediaTiming::new(time, animation).unwrap();
                assert_eq!(timing.time(), time);
                assert_eq!(timing.animation(), animation);
                assert_eq!(timing.payload(), [time, animation]);
            }
        }
        assert_eq!(
            MediaTiming::new(3, 0).unwrap_err(),
            ProtocolError::MediaTimingOutOfRange(3)
        );
        assert_eq!(
            MediaTiming::new(0, 3).unwrap_err(),
            ProtocolError::MediaTimingOutOfRange(3)
        );
        assert_eq!(set_media_timing(2, 2).unwrap().payload, vec![2, 2]);
    }

    #[test]
    fn media_timing_decodes_to_the_ui_labels() {
        assert_eq!(MediaTiming::new(0, 0).unwrap().switch_seconds(), 3);
        assert_eq!(MediaTiming::new(1, 0).unwrap().switch_seconds(), 5);
        assert_eq!(MediaTiming::new(2, 0).unwrap().switch_seconds(), 7);
        assert_eq!(
            MediaTiming::new(0, 0).unwrap().animation_name(),
            "Split Display"
        );
        assert_eq!(MediaTiming::new(0, 1).unwrap().animation_name(), "Scroll");
        assert_eq!(
            MediaTiming::new(0, 2).unwrap().animation_name(),
            "Fade In/Out"
        );
    }

    #[test]
    fn media_timing_routes_to_0x07_on_the_init_pipe() {
        let packet = set_media_timing(1, 2).unwrap();
        assert_eq!(packet.command, Command::InitStep07);
        assert_eq!(packet.code(), 0x07);
        assert_eq!(packet.pipe(), Pipe::Init);
        assert_eq!(packet.packet().unwrap()[2], 0x07);
    }
}
