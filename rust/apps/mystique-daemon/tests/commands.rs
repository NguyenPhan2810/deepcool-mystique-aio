//! The command surface: shapes, validation, screen semantics and a live status read.

mod support;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use mystique_ipc::{cmd, CommandOutcome, OpenResult, RawPacketOutcome, StatusDto};
use mystique_sensors::SensorSample;

use support::{build_core, fast_config, request, ProviderState};

fn open_core(state: &Arc<ProviderState>, now: Instant) -> mystique_daemon::core::Core {
    let mut core = build_core(state.clone(), SensorSample::default(), fast_config(), now);
    let events = core.tick(now);
    assert!(!events.is_empty());
    assert!(core.is_open());
    core
}

#[test]
fn screen_off_is_brightness_zero_and_on_restores_the_last_lit_value() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = open_core(&state, now);

    let (response, _) = core.handle(
        &request(
            1,
            cmd::SET_BRIGHTNESS,
            serde_json::json!({ "brightness": 44 }),
        ),
        now,
    );
    let outcome: CommandOutcome = response.into_data().unwrap();
    assert_eq!(outcome.settings.brightness, 44);
    assert_eq!(state.last_brightness(), Some(44));

    let (response, _) = core.handle(
        &request(2, cmd::SET_SCREEN, serde_json::json!({ "enabled": false })),
        now,
    );
    let outcome: CommandOutcome = response.into_data().unwrap();
    assert!(!outcome.settings.screen_on);
    assert_eq!(outcome.settings.brightness, 0);
    assert_eq!(
        outcome.settings.screen_restore_brightness, 44,
        "the last lit value is remembered"
    );
    assert_eq!(state.last_brightness(), Some(0));

    let (response, _) = core.handle(
        &request(3, cmd::SET_SCREEN, serde_json::json!({ "enabled": true })),
        now,
    );
    let outcome: CommandOutcome = response.into_data().unwrap();
    assert!(outcome.settings.screen_on);
    assert_eq!(outcome.settings.brightness, 44);
    assert_eq!(state.last_brightness(), Some(44));
}

#[test]
fn screen_on_before_any_brightness_restores_the_default() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = open_core(&state, now);

    let (response, _) = core.handle(
        &request(1, cmd::SET_SCREEN, serde_json::json!({ "enabled": false })),
        now,
    );
    assert!(response.ok);
    let (response, _) = core.handle(
        &request(2, cmd::SET_SCREEN, serde_json::json!({ "enabled": true })),
        now,
    );
    let outcome: CommandOutcome = response.into_data().unwrap();
    assert_eq!(outcome.settings.brightness, 70);
}

#[test]
fn set_areas_puts_style_at_index_one_and_old_args_default_to_normal() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = open_core(&state, now);
    let frames_before = state.frames_sent();

    let (response, _) = core.handle(
        &request(
            1,
            cmd::SET_AREAS,
            serde_json::json!({ "main": 0, "style": 1, "aux": 2 }),
        ),
        now,
    );
    assert!(response.ok);
    let outcome: CommandOutcome = response.into_data().unwrap();
    assert_eq!(outcome.settings.style_area, 1);
    assert_eq!(
        state.last_payload(0x04).as_deref(),
        Some([0x00, 0x01, 0x00, 0x02].as_slice())
    );
    // The layout write is followed by one immediate frame: a style only renders once a
    // 0x01 arrives, and it must not wait for the stream's cadence.
    assert_eq!(outcome.packets.len(), 2);
    assert_eq!(outcome.packets[1].code, Some(0x01));
    assert_eq!(outcome.packets[1].pipe, "data");
    assert_eq!(
        state.frames_sent(),
        frames_before,
        "a redraw is not a stream frame and does not move the counter"
    );

    // A client written before Style Type existed omits the field; it defaults to 0.
    let (response, _) = core.handle(
        &request(
            2,
            cmd::SET_AREAS,
            serde_json::json!({ "main": 5, "aux": 1 }),
        ),
        now,
    );
    assert!(response.ok);
    assert_eq!(
        state.last_payload(0x04).as_deref(),
        Some([0x05, 0x00, 0x00, 0x01].as_slice())
    );
}

#[test]
fn rotation_always_clears_the_gyro_in_the_same_payload() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = open_core(&state, now);

    let (response, _) = core.handle(
        &request(1, cmd::SET_ROTATION, serde_json::json!({ "rotation": 2 })),
        now,
    );
    assert!(response.ok);
    let payload = state.last_payload(0x02).expect("a 0x02 write");
    assert_eq!(&payload[..5], &[0x01, 0x00, 0x02, 0x00, 0x46]);
    assert_eq!(
        core.settings().rotation_degrees,
        180,
        "the session follows the write"
    );
}

#[test]
fn bad_arguments_are_a_range_error_not_a_disconnect() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = open_core(&state, now);

    let (response, _) = core.handle(
        &request(
            1,
            cmd::SET_BRIGHTNESS,
            serde_json::json!({ "brightness": 200 }),
        ),
        now,
    );
    assert!(!response.ok);
    assert_eq!(response.error.unwrap().code, "range");
    assert!(core.is_open(), "a range error must not drop the device");

    let (response, _) = core.handle(
        &request(2, cmd::SET_BRIGHTNESS, serde_json::json!({ "wrong": 10 })),
        now,
    );
    assert!(!response.ok);
    assert_eq!(response.error.unwrap().code, "range");
    assert!(core.is_open());
}

#[test]
fn an_unknown_command_is_answered_with_the_id_and_no_disconnect() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = open_core(&state, now);

    let (response, _) = core.handle(&request(77, "set_everything", serde_json::json!({})), now);
    assert_eq!(response.id, 77);
    assert!(!response.ok);
    let error = response.error.unwrap();
    assert_eq!(error.code, "unknown_command");
    assert!(error.hint.unwrap_or_default().contains("set_brightness"));
    assert!(core.is_open());
}

#[test]
fn a_settings_command_without_a_device_reports_not_open() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = build_core(state, SensorSample::default(), fast_config(), now);

    // No tick: the daemon has not acquired anything yet.
    let (response, _) = core.handle(
        &request(
            1,
            cmd::SET_AREAS,
            serde_json::json!({ "main": 5, "aux": 2 }),
        ),
        now,
    );
    assert!(!response.ok);
    assert_eq!(response.error.unwrap().code, "not_open");
}

#[test]
fn status_performs_a_live_status_read_and_reports_it_as_advisory() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = open_core(&state, now);

    let (response, _) = core.handle(&request(1, cmd::STATUS, serde_json::json!({})), now);
    let status: StatusDto = response.into_data().unwrap();
    assert_eq!(status.rotation_source, "0x10 reply (advisory)");
    assert_eq!(status.device.rotation, Some(2));
    assert_eq!(status.rotation_degrees, Some(180));
    assert_eq!(status.mode.as_deref(), Some("machine-info"));
    assert_eq!(status.mode_raw, Some(0xFF));
    assert_eq!(status.result.as_deref(), Some("success"));
    assert!(status
        .packet_hex
        .unwrap_or_default()
        .starts_with("AA 2E 10"));
    assert_eq!(state.status_reads.load(Ordering::SeqCst), 1);
}

#[test]
fn a_failed_status_read_drops_the_handle() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = open_core(&state, now);
    state.status_failures.store(1, Ordering::SeqCst);

    let (response, _) = core.handle(&request(1, cmd::STATUS, serde_json::json!({})), now);
    assert!(!response.ok);
    let error = response.error.unwrap();
    assert_eq!(error.code, "disconnected");
    assert_eq!(error.packets.len(), 1, "the attempted 0x10 is reported");
    assert!(!core.is_open());
}

#[test]
fn open_device_is_idempotent_and_reports_the_descriptor() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = open_core(&state, now);

    let (response, _) = core.handle(&request(1, cmd::OPEN_DEVICE, serde_json::json!({})), now);
    let open: OpenResult = response.into_data().unwrap();
    assert!(open.already_open);
    assert!(open.endpoints_ok);

    let (response, _) = core.handle(&request(2, cmd::DEVICE_STATE, serde_json::json!({})), now);
    let state_dto: mystique_ipc::DeviceState = response.into_data().unwrap();
    assert!(state_dto.device_open);
    assert_eq!(state_dto.settings.brightness, 70);
    let telemetry = state_dto.telemetry.expect("the daemon reports its stream");
    assert!(telemetry.running);
    assert_eq!(telemetry.frames_sent, 1);
}

#[test]
fn a_rejected_command_keeps_the_handle() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = open_core(&state, now);
    state.rejected_commands.store(1, Ordering::SeqCst);

    let (response, _) = core.handle(
        &request(
            1,
            cmd::SET_BRIGHTNESS,
            serde_json::json!({ "brightness": 40 }),
        ),
        now,
    );
    assert!(!response.ok);
    assert_eq!(response.error.unwrap().code, "rejected");
    assert!(
        core.is_open(),
        "a rejection is answered by the device; the link is not lost"
    );
    assert!(core.state().last_error.is_some());
}

#[test]
fn plan_upload_reports_a_missing_file_without_a_device() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = build_core(state, SensorSample::default(), fast_config(), now);

    let (response, _) = core.handle(
        &request(
            1,
            cmd::PLAN_UPLOAD,
            serde_json::json!({ "path": "does-not-exist.png" }),
        ),
        now,
    );
    assert!(!response.ok);
    assert_eq!(response.error.unwrap().code, "io");
}

#[test]
fn erase_sends_0x14_alone_and_needs_the_open_device() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = build_core(state.clone(), SensorSample::default(), fast_config(), now);

    let (response, _) = core.handle(&request(1, cmd::ERASE_IMAGES, serde_json::json!({})), now);
    assert!(!response.ok);
    assert_eq!(response.error.unwrap().code, "not_open");

    let mut core = open_core(&state, now);
    let (response, _) = core.handle(&request(2, cmd::ERASE_IMAGES, serde_json::json!({})), now);
    let outcome: CommandOutcome = response.into_data().unwrap();
    assert_eq!(outcome.packets.len(), 1);
    assert_eq!(outcome.packets[0].code, Some(0x14));
    assert_eq!(outcome.packets[0].label, "0x14 erase-images");
    assert_eq!(outcome.packets[0].payload_hex, "");
    assert!(outcome.message.contains("0x14"));
    assert_eq!(state.last_payload(0x14), Some(Vec::new()));
}

#[test]
fn idle_and_rgb_are_written_at_their_bits_and_survive_other_0x02_writes() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = open_core(&state, now);

    let (response, _) = core.handle(
        &request(
            1,
            cmd::SET_IDLE_SCREEN,
            serde_json::json!({ "enabled": false }),
        ),
        now,
    );
    assert!(response.ok);
    let outcome: CommandOutcome = response.into_data().unwrap();
    assert!(!outcome.settings.idle_screen);
    assert_eq!(
        state.last_payload(0x02).as_deref(),
        Some([0x00, 0x00, 0x00, 0x00, 0x46].as_slice())
    );

    let (response, _) = core.handle(
        &request(2, cmd::SET_RGB_SYNC, serde_json::json!({ "enabled": true })),
        now,
    );
    assert!(response.ok);
    let outcome: CommandOutcome = response.into_data().unwrap();
    assert!(outcome.settings.rgb_sync);
    assert_eq!(
        state.last_payload(0x02).as_deref(),
        Some([0x00, 0x00, 0x00, 0x00, 0x46].as_slice())
    );

    // A brightness write is still a complete 0x02 payload: idle and RGB survive it.
    let (response, _) = core.handle(
        &request(
            3,
            cmd::SET_BRIGHTNESS,
            serde_json::json!({ "brightness": 44 }),
        ),
        now,
    );
    assert!(response.ok);
    assert_eq!(
        state.last_payload(0x02).as_deref(),
        Some([0x00, 0x00, 0x00, 0x00, 0x2C].as_slice())
    );
    let settings = core.settings();
    assert!(!settings.idle_screen);
    assert!(settings.rgb_sync);
    assert_eq!(settings.brightness, 44);
}

#[test]
fn recording_writes_0x05_then_one_redraw_frame() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = open_core(&state, now);
    let frames_before = state.frames_sent();

    let (response, _) = core.handle(
        &request(
            1,
            cmd::SET_RECORDING,
            serde_json::json!({ "cpu_clock": false, "cpu_temp": true }),
        ),
        now,
    );
    assert!(response.ok);
    let outcome: CommandOutcome = response.into_data().unwrap();
    assert!(!outcome.settings.record_cpu_clock);
    assert!(outcome.settings.record_cpu_temp);
    assert_eq!(outcome.packets.len(), 2);
    assert_eq!(outcome.packets[0].code, Some(0x05));
    assert_eq!(outcome.packets[0].payload_hex, "00 01");
    assert_eq!(outcome.packets[1].code, Some(0x01));
    assert_eq!(
        state.frames_sent(),
        frames_before,
        "a redraw is not a stream frame"
    );
}

#[test]
fn a_reconnect_init_carries_idle_rgb_and_recording_state() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = open_core(&state, now);

    for (id, command, args) in [
        (
            1,
            cmd::SET_IDLE_SCREEN,
            serde_json::json!({ "enabled": false }),
        ),
        (2, cmd::SET_RGB_SYNC, serde_json::json!({ "enabled": true })),
        (3, cmd::SET_ROTATION, serde_json::json!({ "rotation": 2 })),
        (
            4,
            cmd::SET_RECORDING,
            serde_json::json!({ "cpu_clock": true, "cpu_temp": false }),
        ),
    ] {
        let (response, _) = core.handle(&request(id, command, args), now);
        assert!(response.ok, "{command}");
    }

    state.telemetry_failures.store(1, Ordering::SeqCst);
    core.tick(now + std::time::Duration::from_millis(10));
    assert!(!core.is_open());
    core.tick(now + std::time::Duration::from_millis(15));
    assert!(core.is_open(), "the retry reacquires the panel");

    let displays = state.init_displays.lock().unwrap();
    let display = displays.last().copied().expect("an init ran");
    assert!(!display.idle_screen());
    assert!(display.rgb_sync());
    assert_eq!(display.rotation().degrees(), 180);
    let recordings = state.init_recordings.lock().unwrap();
    let recording = recordings.last().copied().expect("an init ran");
    assert!(recording.cpu_clock());
    assert!(!recording.cpu_temp());
}

#[test]
fn the_fan_routing_is_host_side_and_only_pushes_a_frame() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = open_core(&state, now);

    let (response, _) = core.handle(
        &request(
            1,
            cmd::SET_FAN_IO,
            serde_json::json!({ "cpu": "pump", "pump": "cpu" }),
        ),
        now,
    );
    assert!(response.ok);
    let outcome: CommandOutcome = response.into_data().unwrap();
    assert_eq!(outcome.settings.fan_io_cpu, "pump");
    assert_eq!(outcome.settings.fan_io_pump, "cpu");
    assert_eq!(
        outcome.packets.len(),
        1,
        "one redraw frame, no setting packet"
    );
    assert_eq!(outcome.packets[0].code, Some(0x01));

    let (response, _) = core.handle(
        &request(
            2,
            cmd::SET_FAN_IO,
            serde_json::json!({ "cpu": "gpu", "pump": "cpu" }),
        ),
        now,
    );
    assert!(!response.ok);
    assert_eq!(response.error.unwrap().code, "range");
}

#[test]
fn the_mode_selector_writes_0x03_and_rejects_anything_else() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = open_core(&state, now);

    let (response, _) = core.handle(
        &request(1, cmd::SET_MODE, serde_json::json!({ "mode": 3 })),
        now,
    );
    assert!(response.ok);
    assert_eq!(state.last_payload(0x03).as_deref(), Some([0x03].as_slice()));

    let (response, _) = core.handle(
        &request(2, cmd::SET_MODE, serde_json::json!({ "mode": 4 })),
        now,
    );
    assert!(!response.ok);
    assert_eq!(response.error.unwrap().code, "range");
}

#[test]
fn a_raw_dry_run_builds_the_packet_without_a_device() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = build_core(state, SensorSample::default(), fast_config(), now);

    let (response, _) = core.handle(
        &request(
            1,
            cmd::RAW_PACKET,
            serde_json::json!({ "command": 0x10, "payload": [], "pipe": "data", "dry_run": true }),
        ),
        now,
    );
    assert!(response.ok);
    let outcome: RawPacketOutcome = response.into_data().unwrap();
    assert!(outcome.dry_run);
    assert!(!outcome.sent);
    assert_eq!(outcome.pipe, "data");
    assert_eq!(outcome.payload_hex, "");
    assert!(outcome.record.is_none());
    assert_eq!(
        outcome.packet_hex,
        "AA 2E 10 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 \
         00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 48 49 44 43 00 02"
    );
}

#[test]
fn a_raw_send_uses_the_chosen_pipe_and_returns_the_log_record() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = open_core(&state, now);

    let (response, _) = core.handle(
        &request(
            1,
            cmd::RAW_PACKET,
            serde_json::json!({ "command": 0x10, "payload": [], "pipe": "data", "dry_run": false }),
        ),
        now,
    );
    assert!(response.ok);
    let outcome: RawPacketOutcome = response.into_data().unwrap();
    assert!(outcome.sent);
    assert_eq!(outcome.result.as_deref(), Some("success"));
    assert_eq!(outcome.result_code, Some(0));
    assert!(outcome.packet_hex.starts_with("AA 2E 10"));
    let record = outcome.record.expect("a sent packet is logged");
    assert_eq!(record.code, Some(0x10));
    assert_eq!(record.pipe, "data");
    assert_eq!(record.result, "success");
    assert!(record.reply_hex.unwrap_or_default().starts_with("55 2E 10"));

    let raw = state.raw_commands.lock().unwrap();
    assert_eq!(raw.len(), 1);
    assert_eq!(raw[0].0, mystique_protocol::Pipe::Data);
    assert_eq!(raw[0].1, 0x10);
    assert!(raw[0].2.is_empty());
}

#[test]
fn a_raw_send_needs_an_open_device_but_a_bad_pipe_is_a_range_error() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = build_core(state, SensorSample::default(), fast_config(), now);

    let (response, _) = core.handle(
        &request(
            1,
            cmd::RAW_PACKET,
            serde_json::json!({ "command": 0xA1, "payload": [], "pipe": "init", "dry_run": false }),
        ),
        now,
    );
    assert!(!response.ok);
    assert_eq!(response.error.unwrap().code, "not_open");

    let (response, _) = core.handle(
        &request(
            2,
            cmd::RAW_PACKET,
            serde_json::json!({ "command": 0x10, "payload": [], "pipe": "sideways", "dry_run": true }),
        ),
        now,
    );
    assert!(!response.ok);
    assert_eq!(response.error.unwrap().code, "range");
}
