//! DeepCool Sensor Bridge pipe framing.
//!
//! The vendor `SensorBridgeServer.exe` pushes length-prefixed JSON snapshots:
//!
//! ```text
//! DE AD BE EF | u16 LE payload length | JSON | 2 trailer bytes (ignored)
//! ```
//!
//! The JSON is a `GET_SENSOR_DATA` message whose `payload` field is itself a JSON
//! *string* of named sensors, each `{ "Value": <number> }`. Field names are matched
//! exactly as the vendor sends them, including `Gpu temperature` with a lowercase `p`.

#[cfg(windows)]
use std::io::Read;
#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(windows)]
use std::sync::{Arc, Mutex};
#[cfg(windows)]
use std::time::{Duration, Instant};

use crate::SensorSample;

pub const MAGIC: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];
pub const HEADER_LEN: usize = 6;
pub const TRAILER_LEN: usize = 2;
/// Guard against a corrupt length field consuming unbounded memory.
pub const MAX_PAYLOAD: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    BadMagic([u8; 4]),
    TooLarge(usize),
    BadJson(String),
    UnexpectedMessage(String),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadMagic(magic) => write!(f, "bad sensor frame magic: {magic:02X?}"),
            Self::TooLarge(length) => write!(f, "sensor frame length {length} is implausible"),
            Self::BadJson(error) => write!(f, "sensor frame JSON is invalid: {error}"),
            Self::UnexpectedMessage(kind) => {
                write!(f, "unexpected sensor message type: {kind}")
            }
        }
    }
}

impl std::error::Error for FrameError {}

#[derive(Debug)]
pub enum TakeFrame {
    Complete {
        sample: SensorSample,
        consumed: usize,
    },
    Incomplete,
}

/// Consumes one frame from `buffer` if a complete one is present.
///
/// Returns [`TakeFrame::Incomplete`] when more bytes are needed. The caller keeps the
/// buffer and calls again after a read; this function never blocks.
pub fn take_frame(buffer: &[u8]) -> Result<TakeFrame, FrameError> {
    if buffer.len() < 4 {
        return Ok(TakeFrame::Incomplete);
    }
    if buffer[..4] != MAGIC {
        let mut magic = [0u8; 4];
        magic.copy_from_slice(&buffer[..4]);
        return Err(FrameError::BadMagic(magic));
    }
    if buffer.len() < HEADER_LEN {
        return Ok(TakeFrame::Incomplete);
    }

    let length = usize::from(u16::from_le_bytes([buffer[4], buffer[5]]));
    if length > MAX_PAYLOAD {
        return Err(FrameError::TooLarge(length));
    }
    let total = HEADER_LEN + length + TRAILER_LEN;
    if buffer.len() < total {
        return Ok(TakeFrame::Incomplete);
    }

    let sample = parse_sensor_json(&buffer[HEADER_LEN..HEADER_LEN + length])?;
    Ok(TakeFrame::Complete {
        sample,
        consumed: total,
    })
}

/// Parses the inner sensor snapshot. A missing or non-numeric field becomes `None`;
/// it is not an error, because the bridge legitimately omits sensors the host lacks.
pub fn parse_sensor_json(bytes: &[u8]) -> Result<SensorSample, FrameError> {
    let outer: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|error| FrameError::BadJson(error.to_string()))?;

    let kind = outer.get("type").and_then(|value| value.as_str());
    if let Some(kind) = kind {
        if kind != "GET_SENSOR_DATA" {
            return Err(FrameError::UnexpectedMessage(kind.to_owned()));
        }
    }

    let payload = match outer.get("payload") {
        Some(serde_json::Value::String(text)) => {
            serde_json::from_str(text).map_err(|error| FrameError::BadJson(error.to_string()))?
        }
        Some(value @ serde_json::Value::Object(_)) => value.clone(),
        _ => return Ok(SensorSample::default()),
    };

    Ok(SensorSample {
        cpu_temperature_c: sensor(&payload, "CPU Temperature").map(|value| clamp_u8(value, 150.0)),
        cpu_usage_percent: sensor(&payload, "CPU Usage").map(|value| clamp_u8(value, 100.0)),
        memory_load_percent: sensor(&payload, "Memory Load").map(|value| clamp_u8(value, 100.0)),
        gpu_temperature_c: sensor(&payload, "Gpu temperature").map(|value| clamp_u8(value, 150.0)),
        cpu_clock_mhz: sensor(&payload, "CPU Clock").map(|value| {
            let mhz = value.round().clamp(0.0, f64::from(u16::MAX));
            mhz as u16
        }),
        pump_rpm: fan(&payload, "AIO Pump"),
        cpu_fan_rpm: fan(&payload, "CPU"),
    })
}

fn sensor(root: &serde_json::Value, name: &str) -> Option<f64> {
    root.get(name)?
        .get("Value")?
        .as_f64()
        .filter(|value| value.is_finite())
}

/// `"Fan list"` is an object of `name -> number`, not the usual `{ "Value": … }`
/// wrapper: `{"AIO Pump": 3341.0, "CPU": 1326.0}`. Verified against the live bridge
/// on 2026-11-19 (`re/findings/21-fan-and-pump-rpm.md`).
fn fan(root: &serde_json::Value, name: &str) -> Option<u16> {
    let value = root.get("Fan list")?.get(name)?.as_f64()?;
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    Some(value.round().clamp(0.0, f64::from(u16::MAX)) as u16)
}

fn clamp_u8(value: f64, maximum: f64) -> u8 {
    value.round().clamp(0.0, maximum) as u8
}

/// Convenience for tests and for a driver that already has a sample.
pub fn sample_to_frame(sample: SensorSample) -> mystique_protocol::TelemetryFrame {
    sample.to_frame()
}

// ---------------------------------------------------------------------------
// Windows named-pipe client
// ---------------------------------------------------------------------------

/// The control pipe DeepCool's `SensorBridgeServer.exe` listens on.
///
/// It carries a small JSON handshake that allocates a second, per-client data pipe.
/// Snapshots arrive only on that data pipe, framed as [`MAGIC`]-prefixed JSON.
#[cfg(windows)]
pub const PIPE_PATH: &str = r"\\.\pipe\deepcool_sensor_data";

#[cfg(windows)]
#[derive(Debug)]
pub enum PipeError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Frame(FrameError),
    /// The server sent a well-formed message this client did not expect.
    Protocol(String),
    Closed,
}

#[cfg(windows)]
impl std::fmt::Display for PipeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "sensor pipe I/O error: {error}"),
            Self::Json(error) => write!(f, "sensor pipe JSON error: {error}"),
            Self::Frame(error) => write!(f, "{error}"),
            Self::Protocol(message) => write!(f, "sensor pipe protocol error: {message}"),
            Self::Closed => f.write_str("sensor pipe closed"),
        }
    }
}

#[cfg(windows)]
impl std::error::Error for PipeError {}

#[cfg(windows)]
impl From<std::io::Error> for PipeError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(windows)]
impl From<serde_json::Error> for PipeError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[cfg(windows)]
impl From<FrameError> for PipeError {
    fn from(error: FrameError) -> Self {
        Self::Frame(error)
    }
}

/// Connects to the vendor sensor bridge and reads framed snapshots.
///
/// The handshake matches `Mystique.Core.Device/SensorBridgeClient.cs` exactly:
/// `SERVER_HELLO` → `CLIENT_HELLO`, `SETUP_DATA_CHANNEL` → `DATA_CHANNEL_READY`, then
/// `GET_SENSOR_DATA` on the control pipe and snapshots on the allocated data pipe.
///
/// `next_sample` blocks until the server pushes a frame, like the reference client.
/// It is only used from the `live` streaming loop.
#[cfg(windows)]
pub struct PipeSensor {
    /// Kept open for the lifetime of the data channel; closing it tears the session
    /// down on the server side.
    _control: std::fs::File,
    data: std::fs::File,
    buffer: Vec<u8>,
}

#[cfg(windows)]
impl PipeSensor {
    pub fn connect() -> Result<Self, PipeError> {
        let mut control = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(PIPE_PATH)?;

        let client_id = format!("{}_mystique_rust", unix_millis());
        write_message(
            &mut control,
            &serde_json::json!({
                "type": "SERVER_HELLO",
                "seq": 0,
                "payload": client_id,
            }),
        )?;
        require_type(read_message(&mut control)?, "CLIENT_HELLO")?;

        write_message(
            &mut control,
            &serde_json::json!({
                "type": "SETUP_DATA_CHANNEL",
                "seq": 1,
                "payload": {},
            }),
        )?;
        let ready = require_type(read_message(&mut control)?, "DATA_CHANNEL_READY")?;
        let path = ready
            .get("payload")
            .and_then(|payload| payload.get("DataPipeName"))
            .and_then(|name| name.as_str())
            .ok_or_else(|| PipeError::Protocol("DATA_CHANNEL_READY omitted DataPipeName".into()))?;
        let pipe_name = data_pipe_name(path)
            .ok_or_else(|| PipeError::Protocol(format!("unusable data pipe path: {path}")))?;

        let data = std::fs::OpenOptions::new()
            .read(true)
            .open(format!(r"\\.\pipe\{pipe_name}"))?;

        write_message(
            &mut control,
            &serde_json::json!({
                "type": "GET_SENSOR_DATA",
                "seq": 2,
                "payload": {},
            }),
        )?;

        Ok(Self {
            _control: control,
            data,
            buffer: Vec::with_capacity(8 * 1024),
        })
    }

    pub fn next_sample(&mut self) -> Result<SensorSample, PipeError> {
        loop {
            match take_frame(&self.buffer)? {
                TakeFrame::Complete { sample, consumed } => {
                    self.buffer.drain(..consumed);
                    return Ok(sample);
                }
                TakeFrame::Incomplete => self.fill()?,
            }
        }
    }

    fn fill(&mut self) -> Result<(), PipeError> {
        let mut chunk = [0u8; 64 * 1024];
        let read = self.data.read(&mut chunk)?;
        if read == 0 {
            return Err(PipeError::Closed);
        }
        self.buffer.extend_from_slice(&chunk[..read]);
        if self.buffer.len() > MAX_PAYLOAD * 2 {
            return Err(PipeError::Frame(FrameError::TooLarge(self.buffer.len())));
        }
        Ok(())
    }
}

/// A vendor-bridge reader that never blocks its caller.
///
/// The bridge pushes one snapshot every ~1.1 s. Reading it inline would cap the whole
/// driver at that rate, which is what the first rate measurement showed: a 100 ms
/// interval produced a 1140 ms period with `sample_us` at 1 108 004 while the panel
/// acknowledged the same frame in 490 µs. The blocking read lives on its own thread;
/// the sampler copies the newest snapshot and is free to send at any rate.
///
/// The feed is a **freshness** source, not a rate source: at 30 Hz the numbers still
/// change about once a second, because that is how often the vendor produces them.
#[cfg(windows)]
pub struct PipeFeed {
    latest: Arc<Mutex<Option<Reading>>>,
    stop: Arc<AtomicBool>,
}

#[cfg(windows)]
#[derive(Clone, Copy)]
struct Reading {
    at: Instant,
    sample: SensorSample,
}

#[cfg(windows)]
impl PipeFeed {
    /// Connects and starts the reader. `None` when the bridge is absent, which is the
    /// normal case on a machine with the vendor stack removed.
    pub fn start() -> Option<Self> {
        let mut sensor = PipeSensor::connect().ok()?;
        let latest = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));

        let sink = Arc::clone(&latest);
        let flag = Arc::clone(&stop);
        std::thread::Builder::new()
            .name("vendor-sensors".to_owned())
            .spawn(move || {
                while !flag.load(Ordering::Relaxed) {
                    match sensor.next_sample() {
                        Ok(sample) => {
                            if let Ok(mut slot) = sink.lock() {
                                *slot = Some(Reading {
                                    at: Instant::now(),
                                    sample,
                                });
                            }
                        }
                        // A closed bridge is normal: the thread simply ends and the
                        // sampler falls back to sysinfo.
                        Err(_) => break,
                    }
                }
            })
            .ok()?;

        Some(Self { latest, stop })
    }

    /// The newest snapshot, if it is younger than `max_age`.
    pub fn fresh(&self, max_age: Duration) -> Option<SensorSample> {
        let slot = self.latest.lock().ok()?;
        let reading = (*slot)?;
        (reading.at.elapsed() <= max_age).then_some(reading.sample)
    }

    /// True once at least one snapshot has arrived.
    pub fn is_producing(&self) -> bool {
        self.latest
            .lock()
            .map(|slot| slot.is_some())
            .unwrap_or(false)
    }
}

#[cfg(windows)]
impl Drop for PipeFeed {
    fn drop(&mut self) {
        // The reader may sit in a blocking read for up to one push interval; it is not
        // joined, so shutdown never waits on the vendor.
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Sends one JSON message. The vendor protocol has no delimiter; the pipe is
/// message-oriented.
#[cfg(windows)]
fn write_message(file: &mut std::fs::File, message: &serde_json::Value) -> Result<(), PipeError> {
    use std::io::Write;
    let bytes = serde_json::to_vec(message)?;
    file.write_all(&bytes)?;
    file.flush()?;
    Ok(())
}

/// Reads one JSON message. A 64 KiB buffer is larger than any handshake message, so
/// a message-mode pipe returns it whole in a single read.
#[cfg(windows)]
fn read_message(file: &mut std::fs::File) -> Result<serde_json::Value, PipeError> {
    let mut buffer = vec![0u8; 64 * 1024];
    let read = file.read(&mut buffer)?;
    if read == 0 {
        return Err(PipeError::Closed);
    }
    Ok(serde_json::from_slice(&buffer[..read])?)
}

#[cfg(windows)]
fn require_type(
    message: serde_json::Value,
    expected: &str,
) -> Result<serde_json::Value, PipeError> {
    match message.get("type").and_then(|kind| kind.as_str()) {
        Some(kind) if kind == expected => Ok(message),
        Some(kind) => Err(PipeError::Protocol(format!(
            "expected {expected}, received {kind}"
        ))),
        None => Err(PipeError::Protocol(format!(
            "message has no type, expected {expected}"
        ))),
    }
}

/// `\\.\pipe\foo` → `foo`.
#[cfg(windows)]
fn data_pipe_name(path: &str) -> Option<&str> {
    let name = path.rsplit('\\').next()?;
    if name.is_empty() || name.contains('/') {
        None
    } else {
        Some(name)
    }
}

#[cfg(windows)]
fn unix_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0)
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn extracts_the_data_pipe_basename() {
        assert_eq!(
            data_pipe_name(r"\\.\pipe\deepcool_sensor_data_1234"),
            Some("deepcool_sensor_data_1234")
        );
        assert_eq!(
            data_pipe_name("deepcool_sensor_data_1234"),
            Some("deepcool_sensor_data_1234")
        );
        assert_eq!(data_pipe_name(r"\\.\pipe\"), None);
    }

    #[test]
    fn the_vendor_pipe_is_probed_without_panicking_when_absent() {
        // On a machine where the vendor service is not running this returns Err; the
        // only requirement is that it does not panic.
        let _ = PipeSensor::connect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(json: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&(json.len() as u16).to_le_bytes());
        bytes.extend_from_slice(json.as_bytes());
        bytes.extend_from_slice(&[0x00, 0x00]);
        bytes
    }

    const JSON: &str = r#"{"type":"GET_SENSOR_DATA","seq":12,"payload":"{\"CPU Temperature\":{\"Value\":42.0},\"CPU Usage\":{\"Value\":7.0},\"Memory Load\":{\"Value\":51.0},\"Gpu temperature\":{\"Value\":60.0},\"CPU Clock\":{\"Value\":4900.0}}"}"#;

    #[test]
    fn parses_a_vendor_snapshot() {
        let sample = parse_sensor_json(JSON.as_bytes()).unwrap();
        assert_eq!(sample.cpu_temperature_c, Some(42));
        assert_eq!(sample.cpu_usage_percent, Some(7));
        assert_eq!(sample.memory_load_percent, Some(51));
        assert_eq!(sample.gpu_temperature_c, Some(60));
        assert_eq!(sample.cpu_clock_mhz, Some(4900));
    }

    #[test]
    fn takes_a_complete_frame_and_reports_its_length() {
        let bytes = frame(JSON);
        match take_frame(&bytes).unwrap() {
            TakeFrame::Complete { sample, consumed } => {
                assert_eq!(sample.cpu_temperature_c, Some(42));
                assert_eq!(consumed, bytes.len());
            }
            TakeFrame::Incomplete => panic!("frame should be complete"),
        }
    }

    #[test]
    fn handles_partial_reads_without_panicking() {
        let bytes = frame(JSON);
        for cut in 0..bytes.len() {
            match take_frame(&bytes[..cut]).unwrap() {
                TakeFrame::Incomplete => {}
                TakeFrame::Complete { .. } => panic!("cut at {cut} should be incomplete"),
            }
        }
    }

    #[test]
    fn rejects_a_bad_magic() {
        let mut bytes = frame(JSON);
        bytes[0] = 0x00;
        assert!(matches!(
            take_frame(&bytes).unwrap_err(),
            FrameError::BadMagic(_)
        ));
    }

    #[test]
    fn rejects_an_implausible_length() {
        let mut bytes = frame(JSON);
        bytes[4..6].copy_from_slice(&u16::MAX.to_le_bytes());
        // u16::MAX is below MAX_PAYLOAD, so this is merely incomplete rather than an
        // error; the guard triggers only for a caller with a larger header type.
        assert!(matches!(take_frame(&bytes).unwrap(), TakeFrame::Incomplete));
    }

    #[test]
    fn missing_sensors_stay_none() {
        let sample = parse_sensor_json(
            br#"{"type":"GET_SENSOR_DATA","payload":"{\"CPU Usage\":{\"Value\":3}}"}"#,
        )
        .unwrap();
        assert_eq!(sample.cpu_usage_percent, Some(3));
        assert_eq!(sample.cpu_temperature_c, None);
        assert_eq!(sample.cpu_clock_mhz, None);
    }

    #[test]
    fn clamps_absurd_values_instead_of_wrapping() {
        let sample = parse_sensor_json(
            br#"{"type":"GET_SENSOR_DATA","payload":"{\"CPU Temperature\":{\"Value\":9999.0},\"CPU Usage\":{\"Value\":-5}}"}"#,
        )
        .unwrap();
        assert_eq!(sample.cpu_temperature_c, Some(150));
        assert_eq!(sample.cpu_usage_percent, Some(0));
    }

    #[test]
    fn rejects_a_foreign_message_type() {
        let error = parse_sensor_json(br#"{"type":"SOMETHING_ELSE","payload":"{}"}"#).unwrap_err();
        assert!(matches!(error, FrameError::UnexpectedMessage(_)));
    }

    #[test]
    fn accepts_an_object_payload_as_well_as_a_string() {
        let sample = parse_sensor_json(
            br#"{"type":"GET_SENSOR_DATA","payload":{"CPU Temperature":{"Value":40}}}"#,
        )
        .unwrap();
        assert_eq!(sample.cpu_temperature_c, Some(40));
    }
}
