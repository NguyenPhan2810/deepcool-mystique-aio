//! The Machine Info handshake — thirteen commands, in the observed order.
//!
//! Payloads come from the 2026-11-18 USBPcap capture of the vendor app on this 360
//! (`re/captures/upload-decoded.json`), not from the 240 prior art. The order is the
//! same; `0x07` and the init `0x08` are empty on this unit, `0x04` carries the
//! display-area selectors (all zeros in the capture), `0x15`/`0x16` are ASCII text
//! labels, and `0x0A` is a fixed prefix plus an incrementing counter.

use std::time::Duration;

use crate::commands::*;
use crate::display::{DisplayAreas, DisplayConfig, RecordingConfig};
use crate::rotation::Rotation;

/// Settle time after an ordinary init step. The working implementation uses 10 ms
/// and lands the panel in Machine Info mode every time.
pub const SETTLE_STEP: Duration = Duration::from_millis(10);

/// Settle time after the `0x0A` step. Verified indirectly: the reference
/// implementation waits 100 ms and the post-init status read is stable.
pub const SETTLE_MODE_SWITCH: Duration = Duration::from_millis(100);

/// One step of the handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitStep {
    pub command: Command,
    pub payload: Vec<u8>,
    pub settle: Duration,
}

impl InitStep {
    fn new(command: Command, payload: impl Into<Vec<u8>>, settle: Duration) -> Self {
        Self {
            command,
            payload: payload.into(),
            settle,
        }
    }
}

/// The three text labels the handshake writes.
///
/// `0x15` and `0x16` are the CPU and GPU names. The vendor sends full strings, e.g.
/// `Intel Core i9-14900K` and `NVIDIA GeForce RTX 4070`; earlier tests missed the
/// feature because they used three-byte payloads. `0x17` was `--` in every captured
/// block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineLabels {
    pub cpu: String,
    pub gpu: String,
    pub machine: String,
}

impl Default for MachineLabels {
    fn default() -> Self {
        Self {
            cpu: "--".to_owned(),
            gpu: "--".to_owned(),
            machine: "--".to_owned(),
        }
    }
}

impl MachineLabels {
    /// Sets the CPU and GPU names; the third label keeps the vendor's `--` default.
    pub fn new(cpu: impl Into<String>, gpu: impl Into<String>) -> Self {
        Self {
            cpu: cpu.into(),
            gpu: gpu.into(),
            machine: "--".to_owned(),
        }
    }

    /// `0x15` payload: the CPU name, ASCII, truncated to 39 bytes.
    pub fn cpu_payload(&self) -> Vec<u8> {
        label_payload(&self.cpu)
    }

    /// `0x16` payload: the GPU name, ASCII, truncated to 39 bytes.
    pub fn gpu_payload(&self) -> Vec<u8> {
        label_payload(&self.gpu)
    }

    /// `0x17` payload.
    pub fn machine_payload(&self) -> Vec<u8> {
        label_payload(&self.machine)
    }
}

/// The 13-command sequence with the vendor's `--` labels, heartbeat counter 0 and the
/// default display areas (main `00` CPU Frequency, Style Type `00` Normal, aux `00`
/// Voltage Mode).
pub fn machine_info_sequence(rotation: Rotation) -> Vec<InitStep> {
    machine_info_sequence_with(
        rotation,
        &MachineLabels::default(),
        0,
        DisplayAreas::default(),
    )
}

/// The 13-command sequence, in the observed order, with caller-supplied labels, the
/// starting value of the `0x0A` counter and the display areas to carry in `0x04`.
///
/// This helper keeps the captured `0x02` block byte-exact: gyro enabled and brightness
/// 70, with only the rotation replaced. Use [`machine_info_sequence_full`] when the
/// caller has a session [`DisplayConfig`] or [`RecordingConfig`] to preserve.
///
/// The order is not ascending and has been stable across captures; do not sort it.
/// Rotation enters at step 2, as payload byte 2 of `0x02`. `0x04` carries
/// `[main, style, 0, aux]`; the captured block has all three values at `00`. The `0x0A`
/// counter is a caller-supplied sequence number because its purpose is unknown.
pub fn machine_info_sequence_with(
    rotation: Rotation,
    labels: &MachineLabels,
    heartbeat: u16,
    areas: DisplayAreas,
) -> Vec<InitStep> {
    let display = DisplayConfig::default()
        .with_gyro(true)
        .with_rotation(rotation);
    machine_info_sequence_full(
        display,
        RecordingConfig::default(),
        labels,
        heartbeat,
        areas,
    )
}

/// The 13-command sequence with the caller's complete `0x02` and `0x05` state.
///
/// `0x02` is absolute state, so a reconnect must carry the session's idle, gyro,
/// rotation, RGB and brightness values rather than resetting them to the captured
/// defaults. `0x05` carries the two recording graph options. This is the sequence the
/// daemon sends; [`machine_info_sequence`] and [`machine_info_sequence_with`] remain the
/// byte-exact capture helpers.
pub fn machine_info_sequence_full(
    display: DisplayConfig,
    recording: RecordingConfig,
    labels: &MachineLabels,
    heartbeat: u16,
    areas: DisplayAreas,
) -> Vec<InitStep> {
    vec![
        InitStep::new(Command::DeviceInfo, [], SETTLE_STEP),
        InitStep::new(Command::DisplayConfig, display.payload(), SETTLE_STEP),
        InitStep::new(Command::SelectMode, INIT_STEP_03_PAYLOAD, SETTLE_STEP),
        InitStep::new(Command::InitStep04, areas.payload(), SETTLE_STEP),
        InitStep::new(Command::InitStep07, [], SETTLE_STEP),
        InitStep::new(Command::InitStep08, [], SETTLE_STEP),
        InitStep::new(Command::InitStep05, recording.payload(), SETTLE_STEP),
        InitStep::new(Command::InitStep0B, [], SETTLE_STEP),
        InitStep::new(Command::InitStep06, INIT_STEP_06_PAYLOAD, SETTLE_STEP),
        InitStep::new(Command::InitStep15, labels.cpu_payload(), SETTLE_STEP),
        InitStep::new(Command::InitStep16, labels.gpu_payload(), SETTLE_STEP),
        InitStep::new(Command::InitStep17, labels.machine_payload(), SETTLE_STEP),
        InitStep::new(
            Command::ModeSwitch,
            mode_switch_payload(heartbeat),
            SETTLE_MODE_SWITCH,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_is_the_captured_order() {
        let commands: Vec<u8> = machine_info_sequence(Rotation::ZERO)
            .iter()
            .map(|step| step.command.code())
            .collect();
        assert_eq!(
            commands,
            vec![0x12, 0x02, 0x03, 0x04, 0x07, 0x08, 0x05, 0x0B, 0x06, 0x15, 0x16, 0x17, 0x0A]
        );
    }

    #[test]
    fn payloads_are_the_360_capture() {
        let steps = machine_info_sequence(Rotation::ZERO);

        assert_eq!(steps[0].command, Command::DeviceInfo);
        assert!(steps[0].payload.is_empty());
        assert_eq!(steps[1].payload, [0x01, 0x01, 0x00, 0x00, 0x46]);
        assert_eq!(steps[2].payload, [0x01]);
        assert_eq!(
            steps[3].payload,
            [0x00, 0x00, 0x00, 0x00],
            "0x04 is main 00 CPU Frequency, aux 00 Voltage Mode in the capture"
        );
        assert!(steps[4].payload.is_empty(), "0x07 is empty on this 360");
        assert!(
            steps[5].payload.is_empty(),
            "init 0x08 is empty on this 360"
        );
        assert_eq!(steps[6].payload, [0x01, 0x01]);
        assert!(steps[7].payload.is_empty());
        assert_eq!(steps[8].payload, [0x01]);
        assert_eq!(steps[9].payload, b"--");
        assert_eq!(steps[10].payload, b"--");
        assert_eq!(steps[11].payload, b"--");
        // The first captured block ends with EA 07 0B 12 0A 2C 3A.
        assert_eq!(
            steps[12].payload,
            vec![0xEA, 0x07, 0x0B, 0x12, 0x0A, 0x00, 0x00]
        );
    }

    #[test]
    fn init_0x04_carries_the_display_area_layout() {
        let areas = DisplayAreas::new(0x03, 0x01, 0x02).unwrap();
        let steps = machine_info_sequence_with(Rotation::ZERO, &MachineLabels::default(), 0, areas);
        assert_eq!(steps[3].command, Command::InitStep04);
        assert_eq!(steps[3].payload, vec![0x03, 0x01, 0x00, 0x02]);
        assert_eq!(steps[3].payload, areas.payload());

        // The capture's block, and therefore the default, is main 00, style 00, aux 00.
        let default = machine_info_sequence(Rotation::ZERO);
        assert_eq!(default[3].payload, vec![0x00, 0x00, 0x00, 0x00]);
        assert_eq!(default[3].payload, DisplayAreas::default().payload());
    }

    #[test]
    fn the_full_sequence_carries_the_session_display_and_recording_state() {
        let display = DisplayConfig::default()
            .with_idle_screen(false)
            .with_gyro(false)
            .with_rotation(Rotation::new(3).unwrap())
            .with_rgb_sync(true)
            .with_brightness(36)
            .unwrap();
        let recording = RecordingConfig::new(true, false);
        let steps = machine_info_sequence_full(
            display,
            recording,
            &MachineLabels::default(),
            0,
            DisplayAreas::default(),
        );
        assert_eq!(steps[1].payload, vec![0x00, 0x00, 0x03, 0x01, 0x24]);
        assert_eq!(steps[6].payload, vec![0x01, 0x00]);
        // Every other step stays in the captured order and shape.
        assert_eq!(steps.len(), 13);
        assert_eq!(steps[2].payload, INIT_STEP_03_PAYLOAD);
        assert_eq!(steps[7].payload, Vec::<u8>::new());
        assert_eq!(steps[8].payload, INIT_STEP_06_PAYLOAD);
    }

    #[test]
    fn the_captured_helper_remains_byte_exact() {
        let steps = machine_info_sequence_with(
            Rotation::ZERO,
            &MachineLabels::default(),
            0,
            DisplayAreas::default(),
        );
        assert_eq!(steps[1].payload, vec![0x01, 0x01, 0x00, 0x00, 0x46]);
        assert_eq!(steps[6].payload, vec![0x01, 0x01]);
    }

    #[test]
    fn cpu_and_gpu_labels_are_written_as_ascii() {
        let labels = MachineLabels::new("Intel Core i9-14900K", "NVIDIA GeForce RTX 4070");
        let steps =
            machine_info_sequence_with(Rotation::ZERO, &labels, 0x2C3A, DisplayAreas::default());
        assert_eq!(steps[9].payload, b"Intel Core i9-14900K");
        assert_eq!(steps[10].payload, b"NVIDIA GeForce RTX 4070");
        assert_eq!(steps[11].payload, b"--");

        let long = "X".repeat(LABEL_MAX_LEN + 10);
        let steps = machine_info_sequence_with(
            Rotation::ZERO,
            &MachineLabels::new(&long, &long),
            0,
            DisplayAreas::default(),
        );
        assert_eq!(steps[9].payload.len(), LABEL_MAX_LEN);
        assert_eq!(steps[10].payload.len(), LABEL_MAX_LEN);
    }

    #[test]
    fn heartbeat_lands_in_the_last_two_bytes_of_the_mode_switch() {
        let steps = machine_info_sequence_with(
            Rotation::ZERO,
            &MachineLabels::default(),
            0x2D01,
            DisplayAreas::default(),
        );
        assert_eq!(
            steps[12].payload,
            vec![0xEA, 0x07, 0x0B, 0x12, 0x0A, 0x2D, 0x01]
        );
    }

    #[test]
    fn every_step_knows_its_pipe() {
        for step in machine_info_sequence(Rotation::ZERO) {
            // The handshake has always been sent on the init pipe, including the
            // silent 0x0A, which acts there even though it does not echo.
            assert_eq!(step.command.pipe(), crate::packet::Pipe::Init);
            assert_eq!(
                step.command.code() == 0x0A,
                step.command.requires_payload(),
                "only the mode switch needs a payload"
            );
        }
    }

    #[test]
    fn rotation_lands_at_payload_offset_two() {
        for rotation in [0u8, 1, 2, 3] {
            let steps = machine_info_sequence(Rotation::new(rotation).unwrap());
            assert_eq!(steps[1].payload, vec![0x01, 0x01, rotation, 0x00, 0x46]);
        }
    }

    #[test]
    fn settle_times_are_ten_and_one_hundred_milliseconds() {
        let steps = machine_info_sequence(Rotation::ZERO);
        for step in &steps[..12] {
            assert_eq!(step.settle, Duration::from_millis(10));
        }
        assert_eq!(steps[12].settle, Duration::from_millis(100));
    }

    #[test]
    fn all_payloads_fit_in_a_packet() {
        for step in machine_info_sequence(Rotation::ZERO) {
            assert!(step.payload.len() <= crate::packet::MAX_PAYLOAD);
            crate::packet::build(step.command.code(), &step.payload).unwrap();
        }
    }
}
