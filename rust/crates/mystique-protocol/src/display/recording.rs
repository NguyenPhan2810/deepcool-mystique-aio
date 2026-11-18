//! `0x05` — the two recording-mode graph options.
//!
//! `0x05` appears in the init chain as `[01 01]`. The vendor rewrites it continuously
//! in Recording mode, and the owner's two recording toggles moved one bit each
//! (`re/findings/18-rgb-and-recording.md`):
//!
//! | Index | Meaning | `01` | `00` |
//! |---|---|---|---|
//! | 0 | CPU frequency graph | shown | hidden |
//! | 1 | CPU temperature graph | shown | hidden |
//!
//! The capture saw the bits move but did not record the click order. The owner's
//! labelled reproduction UI pins index 0 to CPU frequency and index 1 to CPU
//! temperature; that is the mapping implemented here.

use crate::commands::Command;
use crate::init::SETTLE_STEP;
use crate::upload::CommandPacket;

/// Payload length of `0x05`: one bit per recording option.
pub const RECORDING_OPTIONS_LEN: usize = 2;

/// The two recording graph options carried by `0x05`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordingConfig {
    cpu_clock: bool,
    cpu_temp: bool,
}

impl RecordingConfig {
    /// Both graphs on, the captured init payload `[01 01]`.
    pub const DEFAULT_CPU_CLOCK: bool = true;
    pub const DEFAULT_CPU_TEMP: bool = true;

    pub const fn new(cpu_clock: bool, cpu_temp: bool) -> Self {
        Self {
            cpu_clock,
            cpu_temp,
        }
    }

    /// `0x05` index 0. `true` draws the CPU frequency graph.
    pub const fn cpu_clock(&self) -> bool {
        self.cpu_clock
    }

    /// `0x05` index 1. `true` draws the CPU temperature graph.
    pub const fn cpu_temp(&self) -> bool {
        self.cpu_temp
    }

    pub const fn with_cpu_clock(mut self, shown: bool) -> Self {
        self.cpu_clock = shown;
        self
    }

    pub const fn with_cpu_temp(mut self, shown: bool) -> Self {
        self.cpu_temp = shown;
        self
    }

    /// Two bytes, `[cpu_clock, cpu_temp]`, each `00` or `01`.
    pub const fn payload(&self) -> [u8; RECORDING_OPTIONS_LEN] {
        [
            if self.cpu_clock { 0x01 } else { 0x00 },
            if self.cpu_temp { 0x01 } else { 0x00 },
        ]
    }

    pub fn packet(&self) -> CommandPacket {
        CommandPacket::new(Command::InitStep05, self.payload(), SETTLE_STEP)
    }
}

impl Default for RecordingConfig {
    /// The captured init payload `[01 01]`: both graphs on.
    fn default() -> Self {
        Self {
            cpu_clock: Self::DEFAULT_CPU_CLOCK,
            cpu_temp: Self::DEFAULT_CPU_TEMP,
        }
    }
}

/// Sets the two `0x05` recording options.
///
/// A `0x05` change only affects what the next `0x01` frame draws. The daemon pushes one
/// immediate frame after this write, the same way it does after a display-area change,
/// so the owner sees the result without waiting for the stream.
pub fn set_recording(cpu_clock: bool, cpu_temp: bool) -> CommandPacket {
    RecordingConfig::new(cpu_clock, cpu_temp).packet()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::Pipe;

    #[test]
    fn defaults_are_the_captured_init_payload() {
        let config = RecordingConfig::default();
        assert!(config.cpu_clock());
        assert!(config.cpu_temp());
        assert_eq!(config.payload(), [0x01, 0x01]);
        assert_eq!(crate::commands::INIT_STEP_05_PAYLOAD, config.payload());
    }

    #[test]
    fn each_option_is_one_independent_bit() {
        for (clock, temp) in [(true, true), (true, false), (false, true), (false, false)] {
            let config = RecordingConfig::new(clock, temp);
            assert_eq!(config.cpu_clock(), clock);
            assert_eq!(config.cpu_temp(), temp);
            assert_eq!(config.payload(), [u8::from(clock), u8::from(temp)]);
        }
    }

    #[test]
    fn the_capture_transitions_are_pinned() {
        // re/findings/18-rgb-and-recording.md: 01 00 -> 01 01 -> 00 01 -> 01 00 ...
        assert_eq!(RecordingConfig::new(true, false).payload(), [0x01, 0x00]);
        assert_eq!(RecordingConfig::new(true, true).payload(), [0x01, 0x01]);
        assert_eq!(RecordingConfig::new(false, true).payload(), [0x00, 0x01]);
    }

    #[test]
    fn recording_routes_to_0x05_on_the_init_pipe() {
        let packet = set_recording(false, true);
        assert_eq!(packet.command, Command::InitStep05);
        assert_eq!(packet.payload, vec![0x00, 0x01]);
        assert_eq!(packet.code(), 0x05);
        assert_eq!(packet.pipe(), Pipe::Init);
        assert_eq!(packet.packet().unwrap()[2], 0x05);
    }
}
