//! Session state: the `0x02` payload the daemon last sent, the stream counters, and
//! the settings snapshot the UI mirrors.
//!
//! `0x02` is absolute state. Byte 0, the gyro and brightness have no read-back, so the
//! daemon keeps one [`DisplayConfig`] and sends the complete five-byte payload on every
//! change. **Screen off is brightness 0** — proven on hardware
//! (`re/findings/15-driver-verification.md`) — so the last non-zero brightness is
//! remembered and restored by "on".

use std::time::Duration;

use mystique_ipc::{FrameValues, SettingsState, TelemetryStatus};
use mystique_protocol::{
    DisplayAreas, DisplayConfig, MachineLabels, MediaTiming, RecordingConfig, Rotation,
};
use mystique_sensors::SensorSample;

/// The values the daemon owns for this session. None of these are device reads.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    pub(crate) display: DisplayConfig,
    /// The `0x05` recording graph options, preserved across a reconnect.
    pub(crate) recording: RecordingConfig,
    /// What "screen on" restores. `None` until a non-zero brightness is set.
    pub(crate) screen_restore: Option<u8>,
    /// The `0x04` selectors, reused by every later init so a reconnect cannot reset
    /// the panel to the crate default.
    pub(crate) areas: DisplayAreas,
    pub(crate) media_time: u8,
    pub(crate) media_animation: u8,
    pub(crate) labels: MachineLabels,
    /// Last rotation this session set, or the last `0x10` read.
    pub(crate) last_rotation: Option<u8>,
}

impl Settings {
    /// Seeds the session from the CLI. The machine-info init uses this rotation and
    /// these labels, and every later `0x02` write preserves the rotation.
    pub fn from_startup(rotation: Rotation, labels: MachineLabels) -> Self {
        Self {
            display: DisplayConfig::default().with_rotation(rotation),
            labels,
            last_rotation: Some(rotation.raw()),
            ..Self::default()
        }
    }

    /// The brightness "screen on" restores: the last non-zero value, or the crate
    /// default (70).
    pub fn screen_restore_brightness(&self) -> u8 {
        self.screen_restore
            .filter(|value| *value > 0)
            .unwrap_or(DisplayConfig::DEFAULT_BRIGHTNESS)
    }

    /// Everything a client mirrors into its controls. `rotation` is the state the
    /// daemon set, not a live read; `status` adds the advisory `0x10` value.
    pub fn snapshot(&self) -> SettingsState {
        let timing = MediaTiming::new(self.media_time, self.media_animation).ok();
        let brightness = self.display.brightness();
        SettingsState {
            gyro: self.display.gyro_allowed(),
            rotation: self.display.rotation().raw(),
            rotation_degrees: self.display.rotation().degrees(),
            brightness,
            idle_screen: self.display.idle_screen(),
            rgb_sync: self.display.rgb_sync(),
            record_cpu_clock: self.recording.cpu_clock(),
            record_cpu_temp: self.recording.cpu_temp(),
            screen_on: brightness > 0,
            screen_restore_brightness: self.screen_restore_brightness(),
            main_area: self.areas.main(),
            style_area: self.areas.style(),
            aux_area: self.areas.aux(),
            media_time: self.media_time,
            media_seconds: timing.map_or(0, |value| value.switch_seconds()),
            media_animation: self.media_animation,
            media_animation_name: timing.map_or_else(
                || "unknown".to_owned(),
                |value| value.animation_name().to_owned(),
            ),
            cpu_label: self.labels.cpu.clone(),
            gpu_label: self.labels.gpu.clone(),
        }
    }
}

/// A label the panel treats as ASCII: non-empty, ASCII only, at most 39 bytes.
pub fn validate_label(value: &str, which: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{which} label is empty; use \"--\" for no name"));
    }
    if !value.is_ascii() {
        return Err(format!(
            "{which} label must be ASCII; the panel treats the payload as ASCII"
        ));
    }
    if value.len() > mystique_protocol::LABEL_MAX_LEN {
        return Err(format!(
            "{which} label is {} bytes; the maximum is {}",
            value.len(),
            mystique_protocol::LABEL_MAX_LEN
        ));
    }
    Ok(())
}

/// Telemetry counters. `running` is the owner's intent; it survives a device loss, so a
/// reconnect resumes the stream without a new `start_stream`.
#[derive(Debug, Clone)]
pub struct StreamStatus {
    interval: Duration,
    running: bool,
    frames_sent: u64,
    source: Option<String>,
    last_frame: Option<FrameValues>,
    missing: Vec<String>,
    last_error: Option<String>,
    consecutive_errors: u32,
}

impl StreamStatus {
    pub fn new(interval: Duration, running: bool) -> Self {
        Self {
            interval,
            running,
            frames_sent: 0,
            source: None,
            last_frame: None,
            missing: Vec::new(),
            last_error: None,
            consecutive_errors: 0,
        }
    }

    pub const fn running(&self) -> bool {
        self.running
    }

    /// Turns the intent on and resets the counters, matching the UI's Start button.
    pub fn start(&mut self) {
        self.running = true;
        self.frames_sent = 0;
        self.source = None;
        self.last_frame = None;
        self.missing.clear();
        self.last_error = None;
        self.consecutive_errors = 0;
    }

    pub fn stop(&mut self) {
        self.running = false;
    }

    pub fn note_frame(&mut self, frame: FrameValues) {
        self.frames_sent = self.frames_sent.saturating_add(1);
        self.missing.clone_from(&frame.missing);
        self.source = Some(frame.source.clone());
        self.last_frame = Some(frame);
        self.last_error = None;
        self.consecutive_errors = 0;
    }

    pub fn note_error(&mut self, message: impl Into<String>) -> u32 {
        self.consecutive_errors = self.consecutive_errors.saturating_add(1);
        self.last_error = Some(message.into());
        self.consecutive_errors
    }

    pub fn status(&self, device_open: bool) -> TelemetryStatus {
        TelemetryStatus {
            running: self.running,
            frames_sent: self.frames_sent,
            interval_ms: self.interval.as_millis() as u64,
            device_open,
            source: self.source.clone(),
            last_frame: self.last_frame.clone(),
            missing: self.missing.clone(),
            last_error: self.last_error.clone(),
            consecutive_errors: self.consecutive_errors,
        }
    }
}

/// Converts a host sample into the DTO. Absent sensors stay `None`; the wire frame
/// substitutes zero only because the protocol has no "unknown".
pub fn frame_values(sample: &SensorSample, source: &str) -> FrameValues {
    FrameValues {
        cpu_temperature_c: sample.cpu_temperature_c,
        cpu_usage_percent: sample.cpu_usage_percent,
        memory_load_percent: sample.memory_load_percent,
        gpu_temperature_c: sample.gpu_temperature_c,
        cpu_clock_mhz: sample.cpu_clock_mhz,
        missing: sample
            .missing_fields()
            .into_iter()
            .map(str::to_owned)
            .collect(),
        source: source.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_protocol_defaults() {
        let settings = Settings::default().snapshot();
        assert!(settings.screen_on);
        assert!(!settings.gyro);
        assert_eq!(settings.rotation, 0);
        assert_eq!(settings.brightness, 70);
        assert!(settings.idle_screen);
        assert!(!settings.rgb_sync);
        assert!(settings.record_cpu_clock);
        assert!(settings.record_cpu_temp);
        assert_eq!(settings.screen_restore_brightness, 70);
        assert_eq!(settings.media_seconds, 3);
        assert_eq!(settings.media_animation_name, "Split Display");
        assert_eq!(settings.cpu_label, "--");
    }

    #[test]
    fn brightness_zero_is_screen_off_and_the_restore_keeps_the_lit_value() {
        let mut settings = Settings::default();
        settings.display = settings.display.with_brightness(0).unwrap();
        settings.screen_restore = Some(44);
        let snapshot = settings.snapshot();
        assert!(!snapshot.screen_on);
        assert_eq!(snapshot.brightness, 0);
        assert_eq!(snapshot.screen_restore_brightness, 44);

        settings.screen_restore = None;
        assert_eq!(settings.screen_restore_brightness(), 70);
        settings.screen_restore = Some(0);
        assert_eq!(settings.screen_restore_brightness(), 70);
    }

    #[test]
    fn startup_seeds_rotation_and_labels() {
        let settings = Settings::from_startup(
            Rotation::new(2).unwrap(),
            MachineLabels::new("CPU NAME", "GPU NAME"),
        );
        let snapshot = settings.snapshot();
        assert_eq!(snapshot.rotation, 2);
        assert_eq!(snapshot.rotation_degrees, 180);
        assert_eq!(snapshot.cpu_label, "CPU NAME");
        assert_eq!(snapshot.gpu_label, "GPU NAME");
        assert_eq!(settings.last_rotation, Some(2));
    }

    #[test]
    fn a_missing_sensor_stays_none_and_is_named() {
        let sample = SensorSample {
            cpu_temperature_c: Some(41),
            ..SensorSample::default()
        };
        let values = frame_values(&sample, "test");
        assert_eq!(values.cpu_temperature_c, Some(41));
        assert_eq!(values.gpu_temperature_c, None);
        assert_eq!(values.cpu_clock_mhz, None);
        assert_eq!(values.missing.len(), 4);
        assert!(values.missing.contains(&"gpu_temperature_c".to_owned()));
    }

    #[test]
    fn the_stream_counts_frames_and_errors_without_stopping() {
        let mut stream = StreamStatus::new(Duration::from_millis(500), true);
        let values = frame_values(&SensorSample::default(), "test");
        stream.note_frame(values.clone());
        assert_eq!(stream.status(false).frames_sent, 1);
        assert_eq!(stream.status(false).interval_ms, 500);

        assert_eq!(stream.note_error("timeout"), 1);
        assert_eq!(stream.note_error("timeout"), 2);
        let status = stream.status(true);
        assert!(status.running);
        assert_eq!(status.consecutive_errors, 2);
        assert_eq!(status.last_error.as_deref(), Some("timeout"));

        stream.note_frame(values);
        assert_eq!(stream.status(true).consecutive_errors, 0);
        assert!(stream.status(true).last_error.is_none());
    }

    #[test]
    fn starting_again_resets_the_counters_but_stopping_keeps_the_last_frame() {
        let mut stream = StreamStatus::new(Duration::from_secs(1), true);
        let values = frame_values(&SensorSample::default(), "test");
        stream.note_frame(values);
        stream.stop();
        assert!(!stream.running());
        assert!(stream.status(false).last_frame.is_some());

        stream.start();
        let status = stream.status(false);
        assert!(status.running);
        assert_eq!(status.frames_sent, 0);
        assert!(status.last_frame.is_none());
    }

    #[test]
    fn labels_are_ascii_and_bounded() {
        assert!(validate_label("--", "CPU").is_ok());
        assert!(validate_label("Intel Core i9-14900K", "CPU").is_ok());
        assert!(validate_label("", "CPU").is_err());
        assert!(validate_label("caf\u{e9}", "CPU").is_err());
        let long = "X".repeat(mystique_protocol::LABEL_MAX_LEN + 1);
        assert!(validate_label(&long, "GPU")
            .unwrap_err()
            .contains("maximum"));
    }
}
