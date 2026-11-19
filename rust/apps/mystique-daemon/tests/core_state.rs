//! The ownership and recovery state machine, driven with a fake panel.

mod support;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mystique_ipc::Event;
use mystique_protocol::{Clock, TIME_HEADER, TIME_PAYLOAD_LEN};
use mystique_sensors::SensorSample;

use support::{build_core, fast_config, has_state_event, telemetry_frame, ProviderState};

const INTERVAL: Duration = Duration::from_millis(10);
const RETRY: Duration = Duration::from_millis(5);

#[test]
fn a_missing_device_is_retried_then_initialised_once() {
    let state = Arc::new(ProviderState::new());
    state.acquire_failures.store(2, Ordering::SeqCst);
    let now = Instant::now();
    let mut core = build_core(state.clone(), SensorSample::default(), fast_config(), now);

    let events = core.tick(now);
    assert!(!core.is_open());
    assert!(has_state_event(&events));
    assert!(core.state().last_error.is_some());

    core.tick(now + RETRY);
    assert!(!core.is_open());
    assert_eq!(state.acquisitions.load(Ordering::SeqCst), 2);
    assert_eq!(state.inits.load(Ordering::SeqCst), 0);

    // The second retry waits 2 x base, so this is the first instant it is due.
    core.tick(now + RETRY + Duration::from_millis(10));
    assert!(core.is_open(), "the third acquisition succeeds");
    assert_eq!(state.acquisitions.load(Ordering::SeqCst), 3);
    assert_eq!(state.inits.load(Ordering::SeqCst), 1);
    assert_eq!(
        state.init_rotations.lock().unwrap().as_slice(),
        &[0],
        "init runs with the session rotation"
    );
    let init_clock = state.init_clocks.lock().unwrap()[0];
    assert_ne!(
        init_clock,
        Clock::zero(),
        "init carries the live local time, not the no-clock value"
    );
    assert!(core.state().last_error.is_none());
}

#[test]
fn the_clock_packet_carries_a_real_calendar_reading() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = build_core(state.clone(), SensorSample::default(), fast_config(), now);
    core.tick(now);

    let payload = state
        .last_payload(0x0A)
        .expect("the first tick sends the clock");
    assert_eq!(payload.len(), TIME_PAYLOAD_LEN);
    assert_eq!(&payload[..2], &TIME_HEADER);
    let (month, day, hour, minute, second) =
        (payload[2], payload[3], payload[4], payload[5], payload[6]);
    assert!((1..=12).contains(&month), "month {month}");
    assert!((1..=31).contains(&day), "day {day}");
    assert!(hour < 24 && minute < 60 && second < 60);
    assert_ne!(
        [month, day, hour, minute, second],
        [0, 0, 0, 0, 0],
        "the packet is the wall clock, not the explicit zero value"
    );
}

#[test]
fn a_telemetry_failure_drops_the_handle_and_reacquires_with_init() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = build_core(state.clone(), SensorSample::default(), fast_config(), now);

    let first = core.tick(now);
    assert!(core.is_open());
    assert_eq!(state.frames_sent(), 1);
    assert!(telemetry_frame(&first).is_some());

    state.telemetry_failures.store(1, Ordering::SeqCst);
    let failed = core.tick(now + INTERVAL);
    assert!(!core.is_open(), "a failed frame releases the handle");
    assert!(has_state_event(&failed));
    assert!(core.state().last_error.is_some());
    assert_eq!(state.inits.load(Ordering::SeqCst), 1);
    assert_eq!(state.frames_sent(), 1);

    core.tick(now + INTERVAL + RETRY);
    assert!(core.is_open());
    assert_eq!(state.inits.load(Ordering::SeqCst), 2, "init is re-run");
    assert_eq!(state.frames_sent(), 2, "the reacquired panel gets a frame");
    assert!(core.state().last_error.is_none());
}

#[test]
fn an_init_failure_is_not_treated_as_an_open_device() {
    let state = Arc::new(ProviderState::new());
    state.init_failures.store(1, Ordering::SeqCst);
    let now = Instant::now();
    let mut core = build_core(state.clone(), SensorSample::default(), fast_config(), now);

    core.tick(now);
    assert!(!core.is_open(), "a half-initialised handle is never kept");
    assert_eq!(state.inits.load(Ordering::SeqCst), 1);

    core.tick(now + RETRY);
    assert!(core.is_open());
    assert_eq!(state.inits.load(Ordering::SeqCst), 2);
}

#[test]
fn unavailable_sensors_are_null_and_named_in_the_event() {
    let state = Arc::new(ProviderState::new());
    let sample = SensorSample {
        cpu_temperature_c: Some(41),
        ..SensorSample::default()
    };
    let now = Instant::now();
    let mut core = build_core(state, sample, fast_config(), now);

    let events = core.tick(now);
    let frame = telemetry_frame(&events).expect("one telemetry event");
    assert_eq!(frame.cpu_temperature_c, Some(41));
    assert_eq!(frame.cpu_usage_percent, None);
    assert_eq!(frame.gpu_temperature_c, None);
    assert_eq!(frame.cpu_clock_mhz, None);
    assert!(frame.missing.contains(&"gpu_temperature_c".to_owned()));
    assert_eq!(frame.missing.len(), 6);
}

#[test]
fn a_clean_shutdown_blanks_the_panel_and_releases_the_handle() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = build_core(state.clone(), SensorSample::default(), fast_config(), now);
    core.tick(now);
    assert!(core.is_open());

    let events = core.shutdown();
    assert!(!core.is_open());
    assert_eq!(state.last_brightness(), Some(0), "shutdown leaves it dark");
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::State { .. })));
    assert!(!core.is_stream_running());
}

#[test]
fn a_wedged_write_uses_the_longer_retry_pause() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = build_core(state.clone(), SensorSample::default(), fast_config(), now);
    core.tick(now);

    state.wedge.store(true, Ordering::SeqCst);
    state.telemetry_failures.store(1, Ordering::SeqCst);
    core.tick(now + INTERVAL);
    assert!(!core.is_open());

    let wait = core
        .next_wait(now + INTERVAL)
        .expect("a retry must be scheduled");
    assert!(
        wait >= Duration::from_millis(50),
        "a write timeout waits at least the wedge pause, got {wait:?}"
    );
}

#[test]
fn a_rejected_telemetry_frame_keeps_the_link() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = build_core(state.clone(), SensorSample::default(), fast_config(), now);
    core.tick(now);
    assert!(core.is_open());

    state.rejected_telemetry.store(1, Ordering::SeqCst);
    let events = core.tick(now + INTERVAL);
    assert!(
        core.is_open(),
        "a rejection is answered by the device; the link is not lost"
    );
    assert_eq!(state.frames_sent(), 1, "the rejected frame is not counted");
    assert!(has_state_event(&events));
    assert!(core.state().last_error.is_some());

    core.tick(now + INTERVAL * 2);
    assert_eq!(state.frames_sent(), 2, "the next frame uses the same link");
    assert_eq!(state.inits.load(Ordering::SeqCst), 1, "no re-init");
    assert!(core.state().last_error.is_none());
}

#[test]
fn stopping_the_stream_halts_frames_and_starting_it_resumes() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = build_core(state.clone(), SensorSample::default(), fast_config(), now);
    core.tick(now);
    assert_eq!(state.frames_sent(), 1);

    let (response, _) = core.handle(
        &support::request(1, mystique_ipc::cmd::STOP_STREAM, serde_json::json!({})),
        now,
    );
    assert!(response.ok);

    core.tick(now + INTERVAL * 5);
    assert_eq!(state.frames_sent(), 1, "no frames while stopped");
    assert!(!core.is_stream_running());

    let (response, _) = core.handle(
        &support::request(2, mystique_ipc::cmd::START_STREAM, serde_json::json!({})),
        now + INTERVAL * 6,
    );
    assert!(response.ok);
    core.tick(now + INTERVAL * 6);
    assert_eq!(state.frames_sent(), 2);
    assert_eq!(
        core.telemetry_status().frames_sent,
        1,
        "start resets counters"
    );
}

#[test]
fn the_stream_rate_changes_without_stopping_the_stream() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = build_core(state.clone(), SensorSample::default(), fast_config(), now);
    core.tick(now);
    assert_eq!(state.frames_sent(), 1);

    let (response, _) = core.handle(
        &support::request(
            1,
            mystique_ipc::cmd::SET_STREAM_RATE,
            serde_json::json!({ "hz": 30 }),
        ),
        now,
    );
    assert!(response.ok);
    assert_eq!(core.telemetry_status().interval_ms, 33);
    assert!(core.is_stream_running(), "the rate change does not stop it");

    // The change is applied at once: the next tick is due immediately, not one old
    // period later.
    core.tick(now);
    assert_eq!(state.frames_sent(), 2);

    let (response, _) = core.handle(
        &support::request(
            2,
            mystique_ipc::cmd::SET_STREAM_RATE,
            serde_json::json!({ "hz": 0 }),
        ),
        now,
    );
    assert!(!response.ok);
    assert_eq!(
        response.error.map(|error| error.code).as_deref(),
        Some(mystique_ipc::codes::RANGE)
    );
    assert_eq!(
        core.telemetry_status().interval_ms,
        33,
        "a rejected rate leaves the current one in place"
    );
}

#[test]
fn closing_explicitly_is_not_undone_by_the_retry_loop() {
    let state = Arc::new(ProviderState::new());
    let now = Instant::now();
    let mut core = build_core(state.clone(), SensorSample::default(), fast_config(), now);
    core.tick(now);
    assert!(core.is_open());

    let (response, _) = core.handle(
        &support::request(1, mystique_ipc::cmd::CLOSE_DEVICE, serde_json::json!({})),
        now,
    );
    assert!(response.ok);
    assert!(!core.is_open());
    assert_eq!(core.next_wait(now), None);

    core.tick(now + Duration::from_secs(30));
    assert!(!core.is_open(), "an explicit close stays closed");
}
