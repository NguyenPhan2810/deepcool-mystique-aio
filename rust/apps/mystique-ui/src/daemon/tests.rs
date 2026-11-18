//! The client's tests, against the fake transport. No hardware and no daemon process.
//!
//! Each test names the behaviour it protects: id matching, event forwarding, reconnect,
//! and the explicit failure states. A failure that the UI must render differently from a
//! healthy link gets its own test.

use std::collections::VecDeque;
use std::time::Duration;

use mystique_ipc::{
    cmd, DeviceState, Event, FrameValues, Request, Response, ServerMessage, SettingsState,
    ShutdownAck,
};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::fake::{answer_with, start, FakeDaemon};
use super::{LinkState, Notice};

async fn wait_for<F>(notices: &mut mpsc::UnboundedReceiver<Notice>, mut predicate: F) -> Notice
where
    F: FnMut(&Notice) -> bool,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(
            remaining > Duration::ZERO,
            "timed out waiting for a matching notice"
        );
        match tokio::time::timeout(remaining, notices.recv()).await {
            Ok(Some(notice)) if predicate(&notice) => return notice,
            Ok(Some(_)) => continue,
            Ok(None) => panic!("the notice channel closed"),
            Err(_) => panic!("timed out waiting for a matching notice"),
        }
    }
}

fn frame() -> FrameValues {
    FrameValues {
        cpu_temperature_c: Some(41),
        cpu_usage_percent: Some(12),
        memory_load_percent: Some(55),
        gpu_temperature_c: None,
        cpu_clock_mhz: Some(4300),
        pump_rpm: None,
        cpu_fan_rpm: None,
        missing: vec!["gpu_temperature_c".to_owned()],
        source: "test".to_owned(),
    }
}

fn device_state() -> DeviceState {
    DeviceState {
        device_open: true,
        device_present: true,
        service_running: false,
        service_supported: true,
        service_output: String::new(),
        last_error: None,
        rotation: None,
        settings: SettingsState::default(),
        telemetry: None,
    }
}

#[tokio::test]
async fn responses_are_matched_by_id_even_in_reverse_order() {
    // The first request is held; the second releases both, and a stray id is mixed in.
    let mut held: VecDeque<Request> = VecDeque::new();
    let fake = FakeDaemon::new(move |request, _| {
        if request.cmd == cmd::STATUS {
            if let Some(first) = held.pop_front() {
                return vec![
                    ServerMessage::Response(
                        Response::success(9_000, &json!({"who": "stray"})).unwrap(),
                    ),
                    ServerMessage::Response(
                        Response::success(request.id, &json!({"who": "second"})).unwrap(),
                    ),
                    ServerMessage::Response(
                        Response::success(first.id, &json!({"who": "first"})).unwrap(),
                    ),
                ];
            }
            held.push_back(request.clone());
            return Vec::new();
        }
        vec![ServerMessage::Response(
            Response::success(request.id, &json!({"who": "other"})).unwrap(),
        )]
    });

    let (client, _notices) = start(&fake, Duration::from_secs(2));
    let first = client.request(cmd::STATUS, json!({}));
    let second = client.request(cmd::STATUS, json!({}));
    let (first, second) = tokio::join!(first, second);

    let first = first.expect("first answered").into_data::<Value>().unwrap();
    let second = second
        .expect("second answered")
        .into_data::<Value>()
        .unwrap();
    assert_eq!(first["who"], "first", "id 1 received its own payload");
    assert_eq!(second["who"], "second", "id 2 received its own payload");
    assert_eq!(fake.requests().len(), 2, "the stray response sent nothing");
}

#[tokio::test]
async fn telemetry_and_state_events_are_forwarded() {
    let fake = FakeDaemon::new(answer_with(json!({"ok": true}))).greeting(vec![
        ServerMessage::Event(Event::telemetry(frame())),
        ServerMessage::Event(Event::state(device_state())),
    ]);
    let (client, mut notices) = start(&fake, Duration::from_secs(2));

    client
        .request(cmd::DEVICE_STATE, json!({}))
        .await
        .expect("the fake answers");

    match wait_for(&mut notices, |notice| {
        matches!(notice, Notice::Telemetry(_))
    })
    .await
    {
        Notice::Telemetry(values) => {
            assert_eq!(values.cpu_temperature_c, Some(41));
            assert_eq!(values.gpu_temperature_c, None);
            assert_eq!(values.missing, ["gpu_temperature_c"]);
        }
        _ => unreachable!("the predicate matched a telemetry notice"),
    }

    match wait_for(&mut notices, |notice| matches!(notice, Notice::State(_))).await {
        Notice::State(state) => {
            assert!(state.device_open);
            assert!(state.telemetry.is_none());
        }
        _ => unreachable!("the predicate matched a state notice"),
    }
}

#[tokio::test]
async fn a_dropped_connection_is_reopened_and_answers_again() {
    let fake = FakeDaemon::new(answer_with(json!({"round": true}))).closing_after(0, 1);
    let (client, mut notices) = start(&fake, Duration::from_millis(500));

    client
        .request(cmd::STATUS, json!({}))
        .await
        .expect("the first connection answers before it closes");

    wait_for(&mut notices, |notice| {
        matches!(notice, Notice::Link(LinkState::Disconnected { .. }))
    })
    .await;
    wait_for(&mut notices, |notice| {
        matches!(notice, Notice::Link(LinkState::Connected { .. }))
    })
    .await;

    let second = tokio::time::timeout(
        Duration::from_secs(3),
        client.request(cmd::DEVICE_STATE, json!({})),
    )
    .await
    .expect("the second connection is opened without another request");
    assert!(second.is_ok(), "a reconnect answers ordinary requests");
    assert!(fake.connections() >= 2);
}

#[tokio::test]
async fn an_absent_daemon_is_an_explicit_state_not_a_hang() {
    let fake = FakeDaemon::absent();
    let (client, mut notices) = start(&fake, Duration::from_millis(300));

    let error = client
        .request(cmd::STATUS, json!({}))
        .await
        .expect_err("nothing is listening");
    assert_eq!(error.code, "daemon_unavailable");
    assert!(error.hint.unwrap_or_default().contains("mystique-daemon"));

    match wait_for(&mut notices, |notice| {
        matches!(notice, Notice::Link(LinkState::Disconnected { .. }))
    })
    .await
    {
        Notice::Link(LinkState::Disconnected { endpoint, .. }) => {
            assert_eq!(endpoint, "fake://mystique-control-v1");
        }
        _ => unreachable!("the predicate matched a disconnect"),
    }
}

#[tokio::test]
async fn an_unanswered_request_times_out_without_pretending_the_link_is_gone() {
    let fake = FakeDaemon::new(|_, _| Vec::new());
    let (client, mut notices) = start(&fake, Duration::from_millis(150));

    let error = client
        .request(cmd::STATUS, json!({}))
        .await
        .expect_err("the fake never answers");
    assert_eq!(error.code, "daemon_timeout");
    assert!(error.message.contains("did not answer"));

    match wait_for(&mut notices, |notice| matches!(notice, Notice::Fault(_))).await {
        Notice::Fault(fault) => assert_eq!(fault.code, "daemon_timeout"),
        _ => unreachable!("the predicate matched a fault"),
    }
}

#[tokio::test]
async fn a_response_for_an_unknown_id_cannot_resolve_a_waiting_request() {
    let fake = FakeDaemon::new(|request, _| {
        vec![ServerMessage::Response(
            Response::success(request.id + 100, &json!({"who": "wrong"})).unwrap(),
        )]
    });
    let (client, _notices) = start(&fake, Duration::from_millis(150));

    let error = client
        .request(cmd::STATUS, json!({}))
        .await
        .expect_err("the only answer carries the wrong id");
    assert_eq!(error.code, "daemon_timeout");
}

#[tokio::test]
async fn the_shutdown_command_travels_over_the_control_link() {
    let fake = FakeDaemon::new(|request, _| {
        vec![ServerMessage::Response(
            Response::success(
                request.id,
                &json!({ "stopping": true, "message": "the daemon is stopping" }),
            )
            .unwrap(),
        )]
    });
    let (client, _notices) = start(&fake, Duration::from_secs(2));

    let ack: ShutdownAck = client
        .request(cmd::SHUTDOWN, json!({}))
        .await
        .expect("the fake acknowledges the stop")
        .into_data()
        .unwrap();
    assert!(ack.stopping);

    let requests = fake.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].cmd, cmd::SHUTDOWN);
}

#[tokio::test]
async fn a_shutdown_answer_arrives_before_the_daemon_closes_the_link() {
    // The UI's explicit stop: the reply is delivered, then the daemon goes away. The
    // client must report both, in that order, and not treat the close as a lost request.
    let fake = FakeDaemon::new(|request, _| {
        vec![ServerMessage::Response(
            Response::success(
                request.id,
                &json!({ "stopping": true, "message": "the daemon is stopping" }),
            )
            .unwrap(),
        )]
    })
    .closing_after(0, 1);
    let (client, mut notices) = start(&fake, Duration::from_millis(500));

    let response = client
        .request(cmd::SHUTDOWN, json!({}))
        .await
        .expect("the stop is acknowledged before the connection closes");
    assert!(response.ok);

    wait_for(&mut notices, |notice| {
        matches!(notice, Notice::Link(LinkState::Disconnected { .. }))
    })
    .await;
}

#[tokio::test]
async fn a_request_queued_before_the_first_connection_still_completes() {
    let fake = FakeDaemon::new(answer_with(json!({"queued": true})));
    let (client, _notices) = start(&fake, Duration::from_secs(2));

    // No sleep: the request is written as soon as the supervisor has a stream.
    let response = client
        .request(cmd::DEVICE_STATE, json!({}))
        .await
        .expect("queued work is not lost");
    let value = response.into_data::<Value>().unwrap();
    assert_eq!(value["queued"], true);
}

#[tokio::test]
async fn a_request_timeout_leaves_the_link_open() {
    let fake = FakeDaemon::new(|request, _| {
        if request.args.get("quiet") == Some(&json!(true)) {
            return Vec::new();
        }
        vec![ServerMessage::Response(
            Response::success(request.id, &json!({"alive": true})).unwrap(),
        )]
    });
    let (client, mut notices) = start(&fake, Duration::from_millis(120));

    let error = client
        .request(cmd::STATUS, json!({ "quiet": true }))
        .await
        .expect_err("the fake never answers this one");
    assert_eq!(error.code, "daemon_timeout");

    // A slow answer is not a lost daemon: the same connection answers the next request.
    let response = client
        .request(cmd::STATUS, json!({}))
        .await
        .expect("the link survived the timeout");
    assert_eq!(response.into_data::<Value>().unwrap()["alive"], true);

    let link = wait_for(&mut notices, |notice| {
        matches!(notice, Notice::Link(LinkState::Connected { .. }))
    })
    .await;
    assert!(matches!(link, Notice::Link(LinkState::Connected { .. })));
}
