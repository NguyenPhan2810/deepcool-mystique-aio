//! Table-driven conformance tests against the captures in `re/fixtures/`.
//!
//! These require no hardware. A failure here means the crate disagrees with a
//! byte-exact capture from the real device, which is always a bug in the crate.

use std::path::PathBuf;

use mystique_protocol::{
    build_packet, checksum16, init::machine_info_sequence_with, parse_status, DeviceInfo,
    DisplayAreas, DisplayMode, MachineLabels, Response, ResultCode, Rotation, DCLD_KIND_ANIMATED,
    DCLD_KIND_STATIC,
};
use serde_json::Value;

fn repo_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(name)
}

fn fixtures_dir() -> PathBuf {
    repo_dir("re/fixtures")
}

fn load(name: &str) -> Value {
    let path = fixtures_dir().join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{} is not JSON: {error}", path.display()))
}

fn load_capture(name: &str) -> Value {
    let path = repo_dir("re/captures").join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{} is not JSON: {error}", path.display()))
}

fn unhex(text: &str) -> Vec<u8> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        compact.chars().all(|c| c.is_ascii_hexdigit()),
        "non-hex fixture input: {compact}"
    );
    assert_eq!(compact.len() % 2, 0, "odd hex length: {compact}");
    (0..compact.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&compact[index..index + 2], 16).unwrap())
        .collect()
}

fn command_of(request: &Value) -> u8 {
    let text = request["command"].as_str().expect("request.command");
    u8::from_str_radix(text.trim_start_matches("0x"), 16).expect("command is hex")
}

fn result_of(response: &Value) -> ResultCode {
    let text = response["resultCode"]
        .as_str()
        .expect("response.resultCode");
    ResultCode::from_raw(u8::from_str_radix(text.trim_start_matches("0x"), 16).unwrap())
}

/// Requests the reference implementation built with its own [build] equivalent.
const CANONICAL: &[&str] = &[
    "01-info-0x12.json",
    "02-status-machine-info-0x10.json",
    "04-telemetry-baseline-0x01.json",
];

#[test]
fn every_canonical_request_rebuilds_byte_for_byte() {
    for fixture in CANONICAL {
        let json = load(fixture);
        let request = &json["request"];
        let expected = unhex(request["hex"].as_str().unwrap());
        let command = command_of(request);
        let payload = &expected[3..42];
        let rebuilt = build_packet(command, payload).unwrap();
        assert_eq!(
            rebuilt.as_slice(),
            expected.as_slice(),
            "{fixture}: 0x{command:02X} packet differs"
        );
    }

    // The 13-step sequence carries the largest set of payloads; test each one.
    let json = load("03-init-sequence.json");
    for step in json["steps"].as_array().unwrap() {
        let request = &step["request"];
        let expected = unhex(request["hex"].as_str().unwrap());
        let command = command_of(request);
        let rebuilt = build_packet(command, &expected[3..42]).unwrap();
        assert_eq!(
            rebuilt.as_slice(),
            expected.as_slice(),
            "{}: 0x{command:02X} differs",
            step["name"].as_str().unwrap()
        );
    }
}

#[test]
fn init_sequence_matches_the_360_capture() {
    // The old fixture 03-init-sequence.json records what the prior-art sequence sent;
    // the 360 capture records what the vendor sends. They differ in 0x02, 0x04, 0x07,
    // 0x08, 0x15, 0x16 and 0x0A. This test pins the vendor block byte-for-byte.
    let json = load_capture("upload-decoded.json");
    let commands = json["commands"].as_array().unwrap();
    let start = commands
        .iter()
        .position(|command| command["cmd"].as_u64() == Some(0x12))
        .expect("the capture starts with 0x12");
    let captured = &commands[start..start + 13];

    let steps = machine_info_sequence_with(
        Rotation::ZERO,
        &MachineLabels::default(),
        0x2C3A,
        DisplayAreas::default(),
    );
    assert_eq!(captured.len(), steps.len());

    for (expected, actual) in captured.iter().zip(steps.iter()) {
        let code = expected["cmd"].as_u64().unwrap() as u8;
        let padded = unhex(expected["payload"].as_str().unwrap());
        assert_eq!(actual.command.code(), code, "command order");
        assert_eq!(
            actual.payload.as_slice(),
            &padded[..actual.payload.len()],
            "0x{code:02X} payload"
        );
        assert!(
            padded[actual.payload.len()..].iter().all(|byte| *byte == 0),
            "0x{code:02X} payload is longer than the captured value"
        );
    }
}

#[test]
fn captured_frames_are_static_or_animated_as_recorded() {
    let json = load_capture("upload-decoded.json");
    let frames = json["frames"].as_array().unwrap();
    assert_eq!(frames.len(), 14, "twelve GIF frames and two statics");

    let mut statics = 0;
    let mut animated = 0;
    for frame in frames {
        assert_eq!(
            frame["header_sum_ok"].as_bool(),
            Some(true),
            "every captured header must pass our checksum"
        );
        let id = frame["content_id"].as_str().unwrap();
        assert_eq!(id, id.to_lowercase(), "vendor content id is lowercase");
        assert_eq!(id.len(), 32);

        let count = frame["frame_count"].as_u64().unwrap() as u8;
        let byte4 = frame["byte4"].as_u64().unwrap() as u8;
        if count == 0 {
            statics += 1;
            assert_eq!(byte4, DCLD_KIND_STATIC);
            assert_eq!(frame["frame_index"].as_u64(), Some(0));
            assert_eq!(frame["delay_ms"].as_u64(), Some(0));
        } else {
            animated += 1;
            assert_eq!(byte4, DCLD_KIND_ANIMATED);
            assert_eq!(frame["frame_count"].as_u64(), Some(12));
            assert_eq!(frame["delay_ms"].as_u64(), Some(858));
        }
    }
    assert_eq!((statics, animated), (2, 12));
}

#[test]
fn every_captured_reply_parses_and_matches_its_recorded_fields() {
    let mut checked = 0;

    for fixture in [
        "02-status-machine-info-0x10.json",
        "04-telemetry-baseline-0x01.json",
        "05-telemetry-checksum-mismatch.json",
        "06-telemetry-marker-mismatch.json",
        "07-checksum-covers-hidc.json",
        "08-status-checksum-mismatch.json",
    ] {
        checked += check_reply(fixture, &load(fixture)["response"]);
    }

    let init = load("03-init-sequence.json");
    for step in init["steps"].as_array().unwrap() {
        if step["response"]["received"].as_bool() == Some(true) {
            checked += check_reply(step["name"].as_str().unwrap(), &step["response"]);
        }
    }

    assert!(checked >= 19, "only {checked} replies were checked");
}

fn check_reply(label: &str, response: &Value) -> usize {
    let Some(hex) = response["hex"].as_str() else {
        // 0x12 replies are templates with a redacted identifier and no checksum.
        assert_eq!(response["received"].as_bool(), Some(true), "{label}");
        return 0;
    };

    let bytes = unhex(hex);
    assert_eq!(bytes.len(), 48, "{label}: reply length");

    let parsed = Response::parse(&bytes)
        .unwrap_or_else(|error| panic!("{label}: captured reply does not parse: {error}"));

    if let Some(echo) = response["commandEcho"].as_str() {
        let expected = u8::from_str_radix(echo.trim_start_matches("0x"), 16).unwrap();
        assert_eq!(parsed.command_echo(), expected, "{label}: echo");
    }
    assert_eq!(parsed.result(), result_of(response), "{label}: result");
    1
}

#[test]
fn status_reply_decodes_to_machine_info() {
    let json = load("02-status-machine-info-0x10.json");
    let response = Response::parse(&unhex(json["response"]["hex"].as_str().unwrap())).unwrap();
    let status = parse_status(&response);

    assert_eq!(status.rotation, 0);
    assert_eq!(status.rotation_degrees(), Some(0));
    assert_eq!(status.raw_mode, 0xFF);
    assert_eq!(status.mode, DisplayMode::MachineInfo);
    assert_eq!(status.result, ResultCode::Ok);
}

#[test]
fn a_rejected_checksum_still_carries_status() {
    // Fixture 08 is the proof that 0x02 is "your sum was wrong", not "ignored".
    let json = load("08-status-checksum-mismatch.json");
    let response = Response::parse(&unhex(json["response"]["hex"].as_str().unwrap())).unwrap();
    let status = parse_status(&response);

    assert_eq!(status.result, ResultCode::BadChecksum);
    assert_eq!(status.rotation, 0);
    assert_eq!(status.raw_mode, 0xFF);
    assert_eq!(status.mode, DisplayMode::MachineInfo);
}

#[test]
fn telemetry_baseline_encodes_to_the_captured_payload() {
    use mystique_protocol::TelemetryFrame;
    let json = load("04-telemetry-baseline-0x01.json");
    let request = unhex(json["request"]["hex"].as_str().unwrap());
    let decoded = &json["decodedPayload"];
    let payload = &request[3..42];

    // Fixture 04 was emitted by our own C# service before 2026-11-19. It wrote the
    // clock fraction into both RPM slots, so the frame literally carries two 3000 RPM
    // readings. The encoder reproduces it only when told those are the fan values —
    // which is the bug, preserved here as a regression, not a vendor fact.
    let pump = u16::from_le_bytes([payload[21], payload[22]]);
    let fan = u16::from_le_bytes([payload[24], payload[25]]);
    assert_eq!((pump, fan), (3000, 3000));

    let frame = TelemetryFrame::new(
        decoded["cpuTemperatureC"].as_u64().unwrap() as u8,
        decoded["cpuUsagePercent"].as_u64().unwrap() as u8,
        decoded["memoryLoadPercent"].as_u64().unwrap() as u8,
        decoded["gpuTemperatureC"].as_u64().unwrap() as u8,
        decoded["cpuClockMhz"].as_u64().unwrap() as u16,
    )
    .with_fans(pump, fan);

    assert_eq!(frame.encode().as_slice(), payload);
}

#[test]
fn checksum_mismatch_fixtures_differ_only_in_the_recorded_way() {
    // 05: checksum +1.
    let json = load("05-telemetry-checksum-mismatch.json");
    let bytes = unhex(json["request"]["hex"].as_str().unwrap());
    let correct = checksum16(&bytes[..46]);
    let recorded = u16::from_le_bytes([bytes[46], bytes[47]]);
    assert_eq!(recorded, correct.wrapping_add(1));
    assert_eq!(
        json["request"]["correctChecksumHex"].as_str().unwrap(),
        format!("0x{correct:04X}")
    );

    // 06: HIDC zeroed, checksum recomputed for that packet.
    let json = load("06-telemetry-marker-mismatch.json");
    let bytes = unhex(json["request"]["hex"].as_str().unwrap());
    assert_eq!(&bytes[42..46], &[0, 0, 0, 0]);
    assert_eq!(
        u16::from_le_bytes([bytes[46], bytes[47]]),
        checksum16(&bytes[..46])
    );

    // 07: checksum computed over 0..41 only.
    let json = load("07-checksum-covers-hidc.json");
    let bytes = unhex(json["request"]["hex"].as_str().unwrap());
    let recorded = u16::from_le_bytes([bytes[46], bytes[47]]);
    assert_eq!(recorded, checksum16(&bytes[..42]));
    assert_ne!(recorded, checksum16(&bytes[..46]));
}

#[test]
fn the_bad_sync_packet_cannot_be_built() {
    let json = load("09-silent-drop-bad-sync.json");
    let bytes = unhex(json["request"]["hex"].as_str().unwrap());
    assert_eq!(
        bytes[0], 0xAB,
        "fixture starts with the deliberately wrong byte"
    );

    // The only builder always writes AA 2E; no argument can reproduce this packet.
    let rebuilt = build_packet(bytes[2], &bytes[3..42]).unwrap();
    assert_eq!(&rebuilt[..2], &[0xAA, 0x2E]);
    assert_ne!(rebuilt.as_slice(), bytes.as_slice());
}

#[test]
fn info_reply_with_a_synthetic_identifier_is_parsed_and_redacted() {
    // The fixture withholds the real identifier and its checksum, so the shape is
    // reproduced here with a placeholder value.
    let mut raw = [0u8; 48];
    raw[0] = 0x55;
    raw[1] = 0x2E;
    raw[2] = 0x12;
    raw[3] = 0x00;
    raw[4..36].copy_from_slice(b"0123456789ABCDEF0123456789ABCDEF");
    raw[42..46].copy_from_slice(b"HIDC");
    let sum = checksum16(&raw[..46]);
    raw[46..48].copy_from_slice(&sum.to_le_bytes());

    let response = Response::parse(&raw).unwrap();
    let info = DeviceInfo::parse(&response).unwrap();
    assert!(info.identifier_is_hex());
    assert!(info.padding_is_zero());
    assert!(!response.redacted_hex().contains("0123456789ABCDEF"));
}
