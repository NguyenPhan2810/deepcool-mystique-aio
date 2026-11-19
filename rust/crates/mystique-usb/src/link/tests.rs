use super::*;
use crate::mock::{MockTransport, ScriptReply, ScriptStep};

fn link() -> Link<MockTransport> {
    Link::new(MockTransport::device())
}

fn jpeg() -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
    bytes.extend_from_slice(b"JFIF\0");
    bytes.extend_from_slice(&[0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00]);
    bytes.extend_from_slice(&[0x42; 128]);
    bytes.extend_from_slice(&[0xFF, 0xD9]);
    bytes
}

#[test]
fn init_runs_thirteen_steps_and_enters_machine_info() {
    let mut link = link();
    let outcomes = link.init(Rotation::ZERO).unwrap();
    assert_eq!(outcomes.len(), 13);
    assert_eq!(
        outcomes.iter().map(|o| o.command).collect::<Vec<_>>(),
        vec![0x12, 0x02, 0x03, 0x04, 0x07, 0x08, 0x05, 0x0B, 0x06, 0x15, 0x16, 0x17, 0x0A]
    );
    // 0x15/0x16/0x17/0x0A echo 0x00 on the real device.
    assert_eq!(outcomes[9].response.command_echo(), 0x00);
    assert_eq!(outcomes[12].response.command_echo(), 0x00);

    let status = link.status().unwrap();
    assert!(status.is_machine_info());
    assert_eq!(status.rotation, 0);
}

#[test]
fn init_sets_the_requested_rotation() {
    let mut link = link();
    link.init(Rotation::new(2).unwrap()).unwrap();
    assert_eq!(link.transport().rotation(), 2);
    assert_eq!(link.status().unwrap().rotation, 2);
}

#[test]
fn init_sends_the_clock_reading_on_the_clock_command() {
    let mut link = link();
    link.init_with(
        Rotation::ZERO,
        &MachineLabels::default(),
        Clock::new(3, 7, 21, 45, 9),
        DisplayAreas::default(),
    )
    .unwrap();
    let payload = link.transport().last_payload(0x0A).unwrap();
    assert_eq!(&payload[..7], &[0xEA, 0x07, 3, 7, 21, 45, 9]);
}

#[test]
fn status_is_advisory_but_parsed() {
    let mut link = link();
    link.init(Rotation::ZERO).unwrap();
    let status = link.status().unwrap();
    assert_eq!(status.raw_mode, 0xFF);
}

#[test]
fn info_is_redacted() {
    let mut link = link();
    let info = link.info().unwrap();
    assert!(info.identifier_is_hex());
    assert!(!format!("{info:?}").contains("0123456789ABCDEF"));
}

#[test]
fn request_11_returns_the_raw_body_and_it_is_zero_filled() {
    let mut link = link();
    let body = link.request_11_body().unwrap();
    assert_eq!(body.len(), 38);
    assert!(body.iter().all(|byte| *byte == 0x00));
    // Sent on the init OUT endpoint, not the data one.
    let writes = link.transport().writes();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].0, 0x01);
    assert_eq!(writes[0].1[2], 0x11);
}

#[test]
fn an_empty_mode_switch_is_refused_before_it_reaches_the_wire() {
    let mut link = link();
    let error = link.send(Command::ModeSwitch, &[]).unwrap_err();
    assert_eq!(
        error,
        DeviceError::Protocol(ProtocolError::PayloadRequired { command: 0x0A })
    );
    assert!(link.transport().writes().is_empty());
}

#[test]
fn a_device_rejection_maps_to_rejected_not_protocol() {
    let mut link = Link::new(MockTransport::scripted(vec![ScriptStep::new(
        0x10,
        vec![],
        ScriptReply::Result(0x02),
    )]));
    let error = link.send(Command::Status, &[]).unwrap_err();
    assert_eq!(error, DeviceError::Rejected(ResultCode::BadChecksum));
}

#[test]
fn a_read_timeout_is_not_a_wedge() {
    let mut link = Link::new(MockTransport::silent());
    let error = link.send(Command::Status, &[]).unwrap_err();
    assert_eq!(
        error,
        DeviceError::Timeout {
            during: Transfer::Read
        }
    );
    assert!(!error.is_wedge());
}

#[test]
fn a_write_timeout_is_a_wedge() {
    let mut link = Link::new(MockTransport::device().wedge_on(0x10));
    let error = link.send(Command::Status, &[]).unwrap_err();
    assert!(error.is_wedge());
}

#[test]
fn a_script_mismatch_is_reported_clearly() {
    let mut link = Link::new(MockTransport::scripted(vec![ScriptStep::new(
        0x12,
        vec![],
        ScriptReply::Ack,
    )]));
    let error = link.send(Command::Status, &[]).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("script mismatch"), "{message}");
}

#[test]
fn a_frame_is_written_as_one_header_transfer_then_64_byte_chunks() {
    use mystique_protocol::{build_upload_plan, Operation, UploadFrame, UploadRequest};
    let jpeg = jpeg();
    assert_eq!(jpeg.len(), 150);
    let request = UploadRequest::new(Rotation::ZERO, vec![UploadFrame::new(jpeg.clone())]);
    let plan = build_upload_plan(&request).unwrap();

    let mut link = link();
    for operation in &plan {
        if matches!(operation, Operation::Header(_) | Operation::Chunk(_)) {
            link.execute(operation).unwrap();
        }
    }

    let writes = link.transport().writes();
    assert_eq!(writes.len(), 4, "one header plus three JPEG chunks");
    assert_eq!(writes[0].0, 0x01);
    assert_eq!(writes[0].1.len(), 64);
    assert_eq!(&writes[0].1[..4], b"DCLd");
    assert_eq!(
        writes[1..]
            .iter()
            .map(|(_, data)| data.len())
            .collect::<Vec<_>>(),
        vec![64, 64, 22]
    );
    assert!(writes[1..].iter().all(|(_, data)| data.len() <= 64));

    let reassembled: Vec<u8> = writes[1..]
        .iter()
        .flat_map(|(_, data)| data.iter().copied())
        .collect();
    assert_eq!(reassembled, jpeg);
}

#[test]
fn an_oversized_or_empty_chunk_is_refused_before_the_wire() {
    let mut link = link();
    let error = link.write_chunk(&[0u8; 65]).unwrap_err();
    assert_eq!(
        error,
        DeviceError::Protocol(ProtocolError::ImageChunkTooLong {
            actual: 65,
            max: 64
        })
    );
    let error = link.write_chunk(&[]).unwrap_err();
    assert_eq!(error, DeviceError::Protocol(ProtocolError::EmptyImageChunk));
    assert!(link.transport().writes().is_empty());
}

#[test]
fn a_full_upload_plan_executes_end_to_end() {
    use mystique_protocol::{build_upload_plan, Operation, UploadFrame, UploadRequest};
    let request = UploadRequest::new(
        Rotation::ZERO,
        vec![UploadFrame::new(jpeg()).with_delay(50)],
    );
    let plan = build_upload_plan(&request).unwrap();

    let mut link = link();
    let mut headers = 0;
    let mut chunks = 0;
    for operation in &plan {
        match operation {
            Operation::Header(_) => headers += 1,
            Operation::Chunk(_) => chunks += 1,
            _ => {}
        }
        link.execute(operation)
            .unwrap_or_else(|error| panic!("{} failed: {error}", operation.label()));
    }
    assert_eq!(headers, 1);
    assert_eq!(chunks, 3);

    let writes = link.transport().writes();
    let frame_writes: Vec<&(u8, Vec<u8>)> = writes
        .iter()
        .filter(|(_, data)| data.starts_with(b"DCLd") || data.starts_with(b"dcldfinish"))
        .collect();
    // One header (64) and one terminator (55), never one combined buffer.
    assert_eq!(frame_writes.len(), 2);
    assert_eq!(frame_writes[0].1.len(), 64);
    assert_eq!(frame_writes[1].1.len(), 55);
    assert!(!writes
        .iter()
        .any(|(_, data)| data.len() > 64 && !data.starts_with(b"\xaa\x2e")));
}
