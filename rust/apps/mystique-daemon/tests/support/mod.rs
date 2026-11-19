//! Shared fakes for the daemon tests. No hardware, no sockets.
//!
//! This module is compiled into several integration-test binaries, so some helpers are
//! unused in one of them; the allow is scoped to test support only.

#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mystique_daemon::core::{Core, CoreConfig};
use mystique_daemon::environment::{HostEnvironment, ServiceSnapshot};
use mystique_daemon::panel::{DeviceProvider, Panel};
use mystique_daemon::state::Settings;
use mystique_ipc::{codes, CommandError, DeviceInfo, Event, FrameValues, Request};
use mystique_protocol::{
    checksum16, Clock, Command, DisplayAreas, DisplayConfig, DisplayMode, MachineLabels, Operation,
    PanelStatus, Pipe, RecordingConfig, Response, ResultCode, Rotation, TelemetryFrame,
};
use mystique_sensors::SensorSample;
use mystique_usb::DeviceError;

/// Everything the tests observe about the fake panel. Shared across reconnects, so a
/// test can see the second acquisition fail where the first succeeded.
#[derive(Default)]
pub struct ProviderState {
    pub acquisitions: AtomicUsize,
    pub acquire_failures: AtomicUsize,
    pub init_failures: AtomicUsize,
    pub telemetry_failures: AtomicUsize,
    /// Fail the next frame with a device rejection instead of a disconnect.
    pub rejected_telemetry: AtomicUsize,
    /// Fail the next command with a device rejection.
    pub rejected_commands: AtomicUsize,
    pub status_failures: AtomicUsize,
    pub inits: AtomicUsize,
    pub status_reads: AtomicUsize,
    /// Make the next telemetry failure a write timeout instead of a disconnect.
    pub wedge: AtomicBool,
    pub commands: Mutex<Vec<(u8, Vec<u8>)>>,
    pub raw_commands: Mutex<Vec<(Pipe, u8, Vec<u8>)>>,
    pub frames: Mutex<Vec<TelemetryFrame>>,
    pub init_rotations: Mutex<Vec<u8>>,
    pub init_displays: Mutex<Vec<DisplayConfig>>,
    pub init_recordings: Mutex<Vec<RecordingConfig>>,
    pub init_labels: Mutex<Vec<MachineLabels>>,
    pub init_areas: Mutex<Vec<DisplayAreas>>,
    pub init_clocks: Mutex<Vec<Clock>>,
}

impl ProviderState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn frames_sent(&self) -> usize {
        self.frames.lock().map(|frames| frames.len()).unwrap_or(0)
    }

    /// Payload of the last packet sent with this command byte.
    pub fn last_payload(&self, code: u8) -> Option<Vec<u8>> {
        self.commands
            .lock()
            .ok()?
            .iter()
            .rev()
            .find(|(sent, _)| *sent == code)
            .map(|(_, payload)| payload.clone())
    }

    /// `0x02` byte 4 of the last display-config packet.
    pub fn last_brightness(&self) -> Option<u8> {
        self.last_payload(0x02)?.get(4).copied()
    }
}

pub struct FakeProvider {
    state: Arc<ProviderState>,
}

impl FakeProvider {
    pub fn new(state: Arc<ProviderState>) -> Self {
        Self { state }
    }
}

impl DeviceProvider for FakeProvider {
    fn acquire(&mut self) -> Result<Box<dyn Panel>, CommandError> {
        self.state.acquisitions.fetch_add(1, Ordering::SeqCst);
        if take_one(&self.state.acquire_failures) {
            return Err(CommandError::new(codes::NOT_FOUND, "fake: no device"));
        }
        Ok(Box::new(FakePanel {
            state: self.state.clone(),
        }))
    }
}

struct FakePanel {
    state: Arc<ProviderState>,
}

impl Panel for FakePanel {
    fn init(
        &mut self,
        display: DisplayConfig,
        recording: RecordingConfig,
        labels: &MachineLabels,
        clock: Clock,
        areas: DisplayAreas,
    ) -> Result<(), DeviceError> {
        self.state.inits.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut rotations) = self.state.init_rotations.lock() {
            rotations.push(display.rotation().raw());
        }
        if let Ok(mut displays) = self.state.init_displays.lock() {
            displays.push(display);
        }
        if let Ok(mut recordings) = self.state.init_recordings.lock() {
            recordings.push(recording);
        }
        if let Ok(mut recorded) = self.state.init_labels.lock() {
            recorded.push(labels.clone());
        }
        if let Ok(mut recorded) = self.state.init_areas.lock() {
            recorded.push(areas);
        }
        if let Ok(mut recorded) = self.state.init_clocks.lock() {
            recorded.push(clock);
        }
        // The labels are part of the handshake; record that they were non-empty.
        debug_assert!(!labels.cpu.is_empty());
        if take_one(&self.state.init_failures) {
            return Err(DeviceError::Disconnected);
        }
        Ok(())
    }

    fn telemetry(&mut self, frame: &TelemetryFrame) -> Result<(), DeviceError> {
        if take_one(&self.state.rejected_telemetry) {
            return Err(DeviceError::Rejected(ResultCode::BadChecksum));
        }
        if take_one(&self.state.telemetry_failures) {
            return Err(if self.state.wedge.load(Ordering::SeqCst) {
                DeviceError::Timeout {
                    during: mystique_usb::Transfer::Write,
                }
            } else {
                DeviceError::Disconnected
            });
        }
        if let Ok(mut frames) = self.state.frames.lock() {
            frames.push(*frame);
        }
        Ok(())
    }

    fn send(&mut self, command: Command, payload: &[u8]) -> Result<Response, DeviceError> {
        if take_one(&self.state.rejected_commands) {
            return Err(DeviceError::Rejected(ResultCode::BadChecksum));
        }
        if let Ok(mut commands) = self.state.commands.lock() {
            commands.push((command.code(), payload.to_vec()));
        }
        Ok(reply(command.code()))
    }

    fn send_raw(
        &mut self,
        pipe: Pipe,
        command: u8,
        payload: &[u8],
    ) -> Result<Response, DeviceError> {
        if take_one(&self.state.rejected_commands) {
            return Err(DeviceError::Rejected(ResultCode::BadChecksum));
        }
        if let Ok(mut commands) = self.state.raw_commands.lock() {
            commands.push((pipe, command, payload.to_vec()));
        }
        Ok(reply(command))
    }

    fn status(&mut self) -> Result<PanelStatus, DeviceError> {
        self.state.status_reads.fetch_add(1, Ordering::SeqCst);
        if take_one(&self.state.status_failures) {
            return Err(DeviceError::Disconnected);
        }
        Ok(PanelStatus {
            rotation: 2,
            raw_mode: 0xFF,
            mode: DisplayMode::MachineInfo,
            result: ResultCode::Ok,
        })
    }

    fn execute(&mut self, _operation: &Operation) -> Result<Option<Response>, DeviceError> {
        Ok(None)
    }

    fn info(&self) -> Option<DeviceInfo> {
        Some(DeviceInfo {
            product: Some("MYSTIQUE".to_owned()),
            bus: 1,
            address: 5,
            release: "0x0246".to_owned(),
            endpoints: Vec::new(),
        })
    }

    fn endpoints_ok(&self) -> bool {
        true
    }
}

pub struct FakeSampler {
    pub sample: SensorSample,
}

impl mystique_sensors::Sampler for FakeSampler {
    fn sample(&mut self) -> SensorSample {
        self.sample
    }

    fn describe(&self) -> &'static str {
        "fake sampler"
    }
}

pub struct FakeEnvironment {
    pub present: bool,
    pub service: ServiceSnapshot,
}

impl Default for FakeEnvironment {
    fn default() -> Self {
        Self {
            present: true,
            service: ServiceSnapshot {
                supported: true,
                running: false,
                output: "fake service query".to_owned(),
            },
        }
    }
}

impl HostEnvironment for FakeEnvironment {
    fn device_present(&mut self) -> bool {
        self.present
    }

    fn service(&mut self) -> ServiceSnapshot {
        self.service.clone()
    }
}

/// 10 ms frames, 5–40 ms retries, a 50 ms wedge pause: fast enough for unit tests,
/// shaped exactly like the real timings.
pub fn fast_config() -> CoreConfig {
    CoreConfig {
        interval: Duration::from_millis(10),
        retry_base: Duration::from_millis(5),
        retry_max: Duration::from_millis(40),
        wedge_retry: Duration::from_millis(50),
        initial_stream: true,
    }
}

pub fn build_core(
    state: Arc<ProviderState>,
    sample: SensorSample,
    config: CoreConfig,
    now: Instant,
) -> Core {
    Core::new(
        Box::new(FakeProvider::new(state)),
        Box::new(FakeSampler { sample }),
        Box::new(FakeEnvironment::default()),
        config,
        Settings::from_startup(Rotation::ZERO, MachineLabels::default()),
        now,
    )
}

pub fn request(id: u64, command: &str, args: serde_json::Value) -> Request {
    Request {
        id,
        cmd: command.to_owned(),
        args,
    }
}

pub fn telemetry_frame(events: &[Event]) -> Option<&FrameValues> {
    events.iter().find_map(|event| match event {
        Event::Telemetry { frame } => Some(frame),
        Event::State { .. } => None,
    })
}

pub fn has_state_event(events: &[Event]) -> bool {
    events
        .iter()
        .any(|event| matches!(event, Event::State { .. }))
}

/// A valid 48-byte success reply for a command. Tests only; a malformed one would be a
/// test bug, not a device quirk.
pub fn reply(echo: u8) -> Response {
    let mut raw = [0u8; 48];
    raw[0] = 0x55;
    raw[1] = 0x2E;
    raw[2] = echo;
    raw[3] = 0x00;
    raw[42..46].copy_from_slice(b"HIDC");
    let checksum = checksum16(&raw[..46]);
    raw[46..48].copy_from_slice(&checksum.to_le_bytes());
    Response::parse(&raw).expect("test reply must be a valid envelope")
}

fn take_one(counter: &AtomicUsize) -> bool {
    if counter.load(Ordering::SeqCst) == 0 {
        return false;
    }
    counter.fetch_sub(1, Ordering::SeqCst);
    true
}
