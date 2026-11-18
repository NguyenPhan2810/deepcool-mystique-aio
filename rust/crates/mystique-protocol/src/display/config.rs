//! `0x02` — the five-byte display configuration block.
//!
//! `0x02` is one absolute five-byte state block with no partial form:
//!
//! ```text
//! index  0         1      2         3        4
//!       idle      gyro   rotation  rgb      brightness
//! ```
//!
//! Every field moved under a labelled vendor action in the 2026-11-18 captures:
//! idle screen and RGB sync in `re/findings/17-mode-and-idle-screen.md` and
//! `re/findings/18-rgb-and-recording.md`, gyro/rotation/brightness in
//! `re/findings/13-settings-decode.md`.
//!
//! # Idle screen is not screen power
//!
//! `0x02[0]` is the **idle screen display** toggle: what the panel does when it would
//! otherwise sleep. It is not screen enable and it does not blank the panel. Brightness
//! `0` is the screen-off control, proven on hardware
//! (`re/findings/15-driver-verification.md`). The old `unknown_byte0` reading is gone.
//!
//! # RGB sync is an LCD flag, not an LED command
//!
//! `0x02[3]` tells the LCD firmware to defer the ring colour to the motherboard. The
//! RGB temperature warning produces no LCD traffic at all and belongs to the LED
//! subsystem, so it is not modelled here.

use crate::commands::{Command, DISPLAY_CONFIG_LEN};
use crate::error::ProtocolError;
use crate::init::SETTLE_STEP;
use crate::rotation::Rotation;
use crate::upload::CommandPacket;

/// Brightness in `0x02` index 4 is `00`–`64` hex, i.e. 0–100 decimal.
pub const BRIGHTNESS_MAX: u8 = 100;

/// The five meaningful bytes of `0x02`, as absolute state.
///
/// The fields are private so an out-of-range brightness cannot reach
/// [`Self::payload`]: [`Self::new`] validates it, and every `with_*` builder preserves
/// a value that was already valid.
///
/// | Index | Meaning | Values | Confidence |
/// |---|---|---|---|
/// | 0 | Idle screen display | `01` enabled, `00` disabled | Verified |
/// | 1 | Gyroscopes allowed | `01` allowed, `00` off | Verified |
/// | 2 | Rotation | `00`–`03` | Verified |
/// | 3 | RGB sync | `01` synced, `00` not | Verified |
/// | 4 | Brightness | `00`–`64` (0–100) | Verified |
///
/// `0x02` is absolute state: every typed operation builds a complete payload through
/// [`Self::payload`], so a standalone brightness write also carries idle, gyro,
/// rotation and RGB at whatever values the caller's config holds. The daemon therefore
/// keeps one `DisplayConfig` for the session and never constructs a one-byte write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayConfig {
    idle: bool,
    gyro: bool,
    rotation: Rotation,
    rgb: bool,
    brightness: u8,
}

impl DisplayConfig {
    /// Idle screen display, the value in the captured block and the vendor default.
    pub const DEFAULT_IDLE_SCREEN: bool = true;
    /// Gyroscopes off, so rotation commands take effect. This is a driver choice, not
    /// a capture fact: the vendor's keepalive block carries `01` here.
    pub const DEFAULT_GYRO: bool = false;
    /// RGB sync on, which is `00` on the wire. The captured vendor init block carries
    /// `00` here, and the panel follows the motherboard at `00` (2026-11-19).
    pub const DEFAULT_RGB_SYNC: bool = true;
    /// `0x46` (70), the value in the first captured block.
    pub const DEFAULT_BRIGHTNESS: u8 = 70;

    /// The one fallible constructor: every field of the `0x02` payload is set here.
    pub fn new(
        idle: bool,
        gyro: bool,
        rotation: Rotation,
        rgb: bool,
        brightness: u8,
    ) -> Result<Self, ProtocolError> {
        if brightness > BRIGHTNESS_MAX {
            return Err(ProtocolError::BrightnessOutOfRange(brightness));
        }
        Ok(Self {
            idle,
            gyro,
            rotation,
            rgb,
            brightness,
        })
    }

    /// `0x02` index 0. The idle screen display toggle; not screen power.
    pub const fn idle_screen(&self) -> bool {
        self.idle
    }

    /// `0x02` index 1. `01` allowed, `00` off.
    pub const fn gyro_allowed(&self) -> bool {
        self.gyro
    }

    /// `0x02` index 2. Cycled 0→1→2→3→0 while the owner clicked the rotation control.
    pub const fn rotation(&self) -> Rotation {
        self.rotation
    }

    /// `0x02` index 3. **`00` defers the ring colour to the motherboard, `01` does not.**
    ///
    /// Observed on the panel on 2026-11-19: with `01` written, the checkbox labelled
    /// "Sync with motherboard" had no effect, and the ring only followed the board at
    /// `00`. The capture in `re/findings/18-rgb-and-recording.md` correlated the byte
    /// with the checkbox but did not pin which click produced which value, so its
    /// "01 = sync on" label was an assumption. The panel settles it: the byte is
    /// inverted relative to the control's name.
    pub const fn rgb_sync(&self) -> bool {
        self.rgb
    }

    /// `0x02` index 4, 0–100. Zero blanks the panel (proven on hardware).
    pub const fn brightness(&self) -> u8 {
        self.brightness
    }

    pub const fn with_idle_screen(mut self, enabled: bool) -> Self {
        self.idle = enabled;
        self
    }

    pub const fn with_gyro(mut self, allowed: bool) -> Self {
        self.gyro = allowed;
        self
    }

    pub const fn with_rotation(mut self, rotation: Rotation) -> Self {
        self.rotation = rotation;
        self
    }

    pub const fn with_rgb_sync(mut self, synced: bool) -> Self {
        self.rgb = synced;
        self
    }

    pub fn with_brightness(self, brightness: u8) -> Result<Self, ProtocolError> {
        Self::new(self.idle, self.gyro, self.rotation, self.rgb, brightness)
    }

    /// The complete five-byte payload. Every setting operation ends here.
    pub fn payload(&self) -> [u8; DISPLAY_CONFIG_LEN] {
        [
            u8::from(self.idle),
            u8::from(self.gyro),
            self.rotation.raw(),
            // Inverted on purpose: the panel follows the motherboard at `00` and keeps
            // its own effect at `01`. See `rgb_sync` for the evidence.
            if self.rgb { 0x00 } else { 0x01 },
            self.brightness,
        ]
    }

    /// A `0x02` command packet on the init pipe, ready for [`crate::Operation`] or
    /// `Link::send`.
    pub fn packet(&self) -> CommandPacket {
        CommandPacket::new(Command::DisplayConfig, self.payload(), SETTLE_STEP)
    }
}

impl Default for DisplayConfig {
    /// Idle `01`, gyro off, rotation 0, RGB `00`, brightness 70.
    fn default() -> Self {
        Self {
            idle: Self::DEFAULT_IDLE_SCREEN,
            gyro: Self::DEFAULT_GYRO,
            rotation: Rotation::ZERO,
            rgb: Self::DEFAULT_RGB_SYNC,
            brightness: Self::DEFAULT_BRIGHTNESS,
        }
    }
}

/// Sets brightness (0–100) in `0x02` index 4, leaving every other field at its default.
///
/// Prefer the daemon's session builder: a standalone call resets the other four fields
/// to their defaults. **Brightness 0 is the screen-off control.**
pub fn set_brightness(brightness: u8) -> Result<CommandPacket, ProtocolError> {
    Ok(DisplayConfig::default()
        .with_brightness(brightness)?
        .packet())
}

/// Sets rotation (0–90–180–270°) in `0x02` index 2.
///
/// Implemented through [`rotate_with_gyro_off`]: the gyro is cleared in the same
/// payload, because the device ignores rotation while it is enabled.
pub fn set_rotation(rotation: Rotation) -> CommandPacket {
    rotate_with_gyro_off(rotation)
}

/// Sets the gyroscope permission in `0x02` index 1, leaving every other field at its
/// default. `true` = allowed, `false` = off.
pub fn set_gyro(allowed: bool) -> CommandPacket {
    DisplayConfig::default().with_gyro(allowed).packet()
}

/// Sets the idle screen display in `0x02` index 0.
///
/// **This is not screen power.** The panel stays lit; brightness 0 is what blanks it.
pub fn set_idle_screen(enabled: bool) -> CommandPacket {
    DisplayConfig::default().with_idle_screen(enabled).packet()
}

/// Sets RGB light effect sync in `0x02` index 3. `true` defers the ring colour to the
/// motherboard; the LCD still draws its own content.
pub fn set_rgb_sync(synced: bool) -> CommandPacket {
    DisplayConfig::default().with_rgb_sync(synced).packet()
}

/// Rotates the panel with the gyroscopes explicitly off in the same `0x02` payload.
///
/// # Device behaviour
///
/// The panel ignores rotation while the gyroscopes are enabled, and the vendor UI
/// disables the rotation control until they are switched off
/// (`re/findings/13-settings-decode.md`, "Behavioural rule discovered by the owner").
/// Sending rotation while the gyro is on looks like a successful command and changes
/// nothing, which is the worst failure mode. This function therefore clears index 1 as
/// part of the same payload, so the state that allows rotation is established in the
/// same write.
///
/// It is the same payload as [`set_rotation`]; the separate name is the point.
pub fn rotate_with_gyro_off(rotation: Rotation) -> CommandPacket {
    DisplayConfig::default()
        .with_gyro(false)
        .with_rotation(rotation)
        .packet()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::display_config_payload;
    use crate::packet::Pipe;

    fn rotation(raw: u8) -> Rotation {
        Rotation::new(raw).unwrap()
    }

    #[test]
    fn defaults_are_the_documented_ones() {
        let config = DisplayConfig::default();
        assert!(config.idle_screen());
        assert!(!config.gyro_allowed());
        assert_eq!(config.rotation(), Rotation::ZERO);
        assert!(config.rgb_sync());
        assert_eq!(config.brightness(), 70);
        assert_eq!(config.payload(), [0x01, 0x00, 0x00, 0x00, 0x46]);
        // The captured helper keeps the vendor's gyro bit so the 360 init block stays
        // byte-exact; the driver default is gyro off.
        assert_eq!(
            display_config_payload(Rotation::ZERO),
            [0x01, 0x01, 0x00, 0x00, 0x46]
        );
    }

    #[test]
    fn every_field_lands_at_its_documented_index() {
        let config = DisplayConfig::new(false, true, rotation(3), true, 100).unwrap();
        assert_eq!(config.payload(), [0x00, 0x01, 0x03, 0x00, 0x64]);

        let config = DisplayConfig::new(true, false, rotation(2), false, 0).unwrap();
        assert_eq!(config.payload(), [0x01, 0x00, 0x02, 0x01, 0x00]);
    }

    #[test]
    fn brightness_accepts_zero_to_one_hundred_and_rejects_above() {
        for brightness in [0, 21, 70, 100] {
            let config = DisplayConfig::default()
                .with_brightness(brightness)
                .unwrap();
            assert_eq!(config.payload()[4], brightness);
        }
        assert_eq!(
            DisplayConfig::default().with_brightness(101).unwrap_err(),
            ProtocolError::BrightnessOutOfRange(101)
        );
        assert_eq!(
            set_brightness(255).unwrap_err(),
            ProtocolError::BrightnessOutOfRange(255)
        );
        assert!(DisplayConfig::new(true, false, Rotation::ZERO, false, 200).is_err());
    }

    #[test]
    fn rotation_is_at_index_two_for_all_four_positions() {
        for raw in 0u8..=3 {
            let packet = set_rotation(rotation(raw));
            assert_eq!(packet.payload, vec![0x01, 0x00, raw, 0x00, 0x46]);
            assert_eq!(packet.payload[2], raw);
            assert_eq!(
                DisplayConfig::default()
                    .with_rotation(rotation(raw))
                    .payload()[2],
                raw
            );
        }
    }

    #[test]
    fn the_standalone_operations_only_move_their_own_bit() {
        assert_eq!(
            set_brightness(0).unwrap().payload,
            vec![0x01, 0x00, 0x00, 0x00, 0x00]
        );
        assert_eq!(
            set_idle_screen(false).payload,
            vec![0x00, 0x00, 0x00, 0x00, 0x46]
        );
        assert_eq!(
            set_rgb_sync(true).payload,
            vec![0x01, 0x00, 0x00, 0x00, 0x46]
        );
        assert_eq!(set_gyro(true).payload, vec![0x01, 0x01, 0x00, 0x00, 0x46]);
        assert_eq!(
            set_rotation(rotation(1)).payload,
            vec![0x01, 0x00, 0x01, 0x00, 0x46]
        );
    }

    #[test]
    fn brightness_zero_is_the_screen_off_payload() {
        // The 2026-11-18 correction: off is a brightness value, not byte 0.
        let packet = set_brightness(0).unwrap();
        assert_eq!(packet.payload[4], 0x00);
        assert_eq!(packet.payload[0], 0x01, "idle screen is not screen power");
        assert_eq!(packet.command, Command::DisplayConfig);
    }

    #[test]
    fn idle_and_rgb_are_verified_independent_bits() {
        // re/findings/17-mode-and-idle-screen.md: 01 01 00 01 46 -> 00 01 00 01 46
        // when Idle Screen Display was turned off, and back.
        let idle_off = DisplayConfig::new(false, true, Rotation::ZERO, true, 70).unwrap();
        assert_eq!(idle_off.payload(), [0x00, 0x01, 0x00, 0x00, 0x46]);
        // re/findings/18-rgb-and-recording.md: 01 01 00 01 46 <-> 01 01 00 00 46
        // six times while the RGB sync checkbox was clicked.
        let rgb_off = DisplayConfig::new(true, true, Rotation::ZERO, false, 70).unwrap();
        assert_eq!(rgb_off.payload(), [0x01, 0x01, 0x00, 0x01, 0x46]);
    }

    #[test]
    fn rotating_always_clears_the_gyro() {
        let packet = rotate_with_gyro_off(rotation(2));
        assert_eq!(packet.payload, vec![0x01, 0x00, 0x02, 0x00, 0x46]);
        assert_eq!(packet.payload[1], 0x00);
        assert_eq!(packet.payload[2], 0x02);
        assert_eq!(set_rotation(rotation(2)), packet);
    }

    #[test]
    fn the_builder_agrees_with_the_captured_helper() {
        for raw in 0u8..=3 {
            assert_eq!(
                DisplayConfig::default()
                    .with_gyro(true)
                    .with_rotation(rotation(raw))
                    .payload(),
                display_config_payload(rotation(raw))
            );
        }
    }

    #[test]
    fn display_config_is_always_five_bytes_on_the_init_pipe() {
        let packet = DisplayConfig::default().packet();
        assert_eq!(packet.payload.len(), DISPLAY_CONFIG_LEN);
        assert_eq!(packet.command, Command::DisplayConfig);
        assert_eq!(packet.pipe(), Pipe::Init);
        let bytes = packet.packet().unwrap();
        assert_eq!(&bytes[..3], &[0xAA, 0x2E, 0x02]);
        assert_eq!(&bytes[3..8], &[0x01, 0x00, 0x00, 0x00, 0x46]);
    }
}
