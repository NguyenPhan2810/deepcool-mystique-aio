//! Unsolicited events broadcast to every connected client.

use serde::{Deserialize, Serialize};

use crate::dto::DeviceState;

/// The five numbers the panel draws, as the daemon read them.
///
/// A field is `null` when the sensor could not be read. The wire frame still carries
/// zero there — the protocol has no "unknown" — but nothing in this DTO pretends the
/// zero is a reading. `missing` repeats that list in the form the UI already displays.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameValues {
    pub cpu_temperature_c: Option<u8>,
    pub cpu_usage_percent: Option<u8>,
    pub memory_load_percent: Option<u8>,
    pub gpu_temperature_c: Option<u8>,
    pub cpu_clock_mhz: Option<u16>,
    /// Field names that were unavailable for this frame.
    #[serde(default)]
    pub missing: Vec<String>,
    /// Which sampler produced the frame, e.g. the vendor pipe or `sysinfo`.
    #[serde(default)]
    pub source: String,
}

/// Everything a client needs to render the stream. `running` is the owner's intent;
/// `device_open` says whether frames can actually flow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelemetryStatus {
    pub running: bool,
    pub frames_sent: u64,
    pub interval_ms: u64,
    pub device_open: bool,
    pub source: Option<String>,
    pub last_frame: Option<FrameValues>,
    /// Values the sampler could not read for the last frame.
    pub missing: Vec<String>,
    pub last_error: Option<String>,
    pub consecutive_errors: u32,
}

/// One unsolicited message. Tagged by `event`, so the wire is exactly
/// `{"event":"telemetry","frame":{...}}` or `{"event":"state","state":{...}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Telemetry { frame: FrameValues },
    State { state: Box<DeviceState> },
}

impl Event {
    pub const fn telemetry(frame: FrameValues) -> Self {
        Self::Telemetry { frame }
    }

    pub fn state(state: DeviceState) -> Self {
        Self::State {
            state: Box::new(state),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::SettingsState;

    fn frame() -> FrameValues {
        FrameValues {
            cpu_temperature_c: Some(41),
            cpu_usage_percent: Some(12),
            memory_load_percent: Some(55),
            gpu_temperature_c: None,
            cpu_clock_mhz: Some(4300),
            missing: vec!["gpu_temperature_c".to_owned()],
            source: "sysinfo".to_owned(),
        }
    }

    #[test]
    fn a_telemetry_event_is_tagged_and_nests_the_frame() {
        let json = serde_json::to_value(Event::telemetry(frame())).unwrap();
        assert_eq!(json["event"], "telemetry");
        assert_eq!(json["frame"]["cpu_temperature_c"], 41);
        assert_eq!(json["frame"]["gpu_temperature_c"], serde_json::Value::Null);
        assert_eq!(json["frame"]["missing"][0], "gpu_temperature_c");
    }

    #[test]
    fn a_state_event_is_tagged_and_nests_the_state() {
        let state = DeviceState {
            device_open: false,
            device_present: true,
            service_running: false,
            service_supported: true,
            service_output: String::new(),
            last_error: None,
            rotation: None,
            settings: SettingsState::default(),
            telemetry: None,
        };
        let json = serde_json::to_value(Event::state(state.clone())).unwrap();
        assert_eq!(json["event"], "state");
        assert_eq!(json["state"]["device_present"], true);
        assert_eq!(json["state"]["settings"]["brightness"], 70);

        let decoded: Event = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, Event::state(state));
    }
}
