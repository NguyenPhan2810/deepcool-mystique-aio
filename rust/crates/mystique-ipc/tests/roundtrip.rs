//! Round-trip and envelope-shape tests for the shared IPC messages. No sockets, no
//! device: this is the contract a UI client and the daemon must agree on.

use mystique_ipc::{
    args, cmd, decode, encode, encode_line, CommandError, DeviceState, Event, FrameValues, Request,
    Response, ServerMessage, SettingsState, UploadPlanSummary, PROTOCOL_VERSION,
};

fn state() -> DeviceState {
    DeviceState {
        device_open: true,
        device_present: true,
        service_running: false,
        service_supported: true,
        service_output: "STATE : 1 STOPPED".to_owned(),
        last_error: None,
        rotation: Some(1),
        settings: SettingsState::default(),
        telemetry: None,
    }
}

fn frame() -> FrameValues {
    FrameValues {
        cpu_temperature_c: Some(52),
        cpu_usage_percent: None,
        memory_load_percent: Some(61),
        gpu_temperature_c: Some(48),
        cpu_clock_mhz: None,
        pump_rpm: None,
        cpu_fan_rpm: None,
        missing: vec!["cpu_usage_percent".to_owned(), "cpu_clock_mhz".to_owned()],
        source: "vendor sensor pipe".to_owned(),
    }
}

#[test]
fn the_protocol_version_is_one() {
    assert_eq!(PROTOCOL_VERSION, 1);
}

#[test]
fn a_request_round_trips_through_the_line_encoder() {
    let request = Request::with_args(
        21,
        cmd::SET_AREAS,
        &args::Areas {
            main: 5,
            style: 1,
            aux: 2,
        },
    )
    .unwrap();
    let line = encode_line(&request).unwrap();
    assert!(line.ends_with('\n'));
    assert_eq!(line.matches('\n').count(), 1, "one frame per message");

    let decoded: Request = decode(line.as_bytes()).unwrap();
    assert_eq!(decoded, request);
    assert_eq!(
        decoded.parse_args::<args::Areas>().unwrap(),
        args::Areas {
            main: 5,
            style: 1,
            aux: 2,
        }
    );
}

#[test]
fn the_request_envelope_is_exactly_id_cmd_args() {
    let request = Request::new(1, cmd::DEVICE_STATE);
    let value = serde_json::to_value(&request).unwrap();
    let object = value.as_object().unwrap();
    assert_eq!(object.len(), 3);
    assert!(object.contains_key("id"));
    assert!(object.contains_key("cmd"));
    assert!(object.contains_key("args"));
}

#[test]
fn a_success_response_round_trips_with_its_dto() {
    let response = Response::success(4, &state()).unwrap();
    let bytes = encode(&response).unwrap();
    let decoded: Response = decode(&bytes).unwrap();
    assert_eq!(decoded, response);
    let state: DeviceState = decoded.into_data().unwrap();
    assert_eq!(state.rotation, Some(1));
}

#[test]
fn a_failure_response_round_trips_with_packets_and_a_hint() {
    let error = CommandError::new("stall", "endpoint stalled during write")
        .with_hint("stop here; a stalled write is not retried immediately");
    let response = Response::failure(5, error.clone());
    let bytes = encode(&response).unwrap();
    let decoded: Response = decode(&bytes).unwrap();
    assert!(!decoded.ok);
    assert_eq!(
        decoded
            .clone()
            .into_data::<serde_json::Value>()
            .unwrap_err(),
        error
    );
}

#[test]
fn a_telemetry_event_round_trips_and_keeps_unavailable_fields_null() {
    let event = Event::telemetry(frame());
    let bytes = encode(&event).unwrap();
    let decoded: Event = decode(&bytes).unwrap();
    assert_eq!(decoded, event);

    let value = serde_json::to_value(&decoded).unwrap();
    assert_eq!(value["event"], "telemetry");
    assert_eq!(value["frame"]["cpu_usage_percent"], serde_json::Value::Null);
    assert_eq!(value["frame"]["missing"][0], "cpu_usage_percent");
}

#[test]
fn a_state_event_round_trips_inside_the_server_message_union() {
    let message = ServerMessage::Event(Event::state(state()));
    let bytes = encode(&message).unwrap();
    let decoded: ServerMessage = decode(&bytes).unwrap();
    assert_eq!(decoded, message);
}

#[test]
fn the_server_union_tells_a_response_from_an_event() {
    let response = ServerMessage::Response(Response::success(1, &frame()).unwrap());
    let event = ServerMessage::Event(Event::state(state()));

    for message in [response, event] {
        let bytes = encode(&message).unwrap();
        let decoded: ServerMessage = decode(&bytes).unwrap();
        assert_eq!(decoded, message);
    }
}

#[test]
fn an_unknown_command_name_parses_as_a_request_so_the_daemon_can_answer_it() {
    // The envelope is deliberately dumb: rejecting an unknown command at the framing
    // layer would give the client no id to match an error to.
    let bytes = b"{\"id\":77,\"cmd\":\"set_everything\",\"args\":{\"all\":true}}\n";
    let decoded: Request = decode(bytes).unwrap();
    assert_eq!(decoded.id, 77);
    assert_eq!(decoded.cmd, "set_everything");
    assert!(!cmd::is_known(&decoded.cmd));
}

#[test]
fn an_upload_plan_summary_round_trips_with_its_optional_preview() {
    let summary = UploadPlanSummary {
        path: "C:\\pictures\\panel.png".to_owned(),
        kind: mystique_ipc::UploadKind::Image,
        fit: "cover".to_owned(),
        source_width: 1920,
        source_height: 1080,
        output_width: 480,
        output_height: 640,
        frames: 1,
        total_bytes: 122,
        chunk_count: 2,
        header_count: 1,
        command_count: 19,
        operations: 23,
        rotation: 2,
        duration_ms: 0,
        content_id: "0123456789abcdef".to_owned(),
        preview: Some("data:image/png;base64,AAAA".to_owned()),
        commands: Vec::new(),
    };
    let response = Response::success(8, &summary).unwrap();
    let decoded: Response = decode(&encode(&response).unwrap()).unwrap();
    assert_eq!(decoded.into_data::<UploadPlanSummary>().unwrap(), summary);
}
