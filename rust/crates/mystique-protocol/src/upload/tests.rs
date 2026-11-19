use super::*;
use crate::dcl::{DCLD_KIND_ANIMATED, DCLD_KIND_STATIC};

fn jpeg_with_body(body_len: usize) -> Vec<u8> {
    // SOI + JFIF APP0 + a dummy payload that is not a real JPEG body. The protocol
    // crate only checks the two markers; decoding is not its job.
    let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
    bytes.extend_from_slice(b"JFIF\0");
    bytes.extend_from_slice(&[0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00]);
    bytes.extend(std::iter::repeat_n(0x42, body_len));
    bytes.extend_from_slice(&[0xFF, 0xD9]);
    bytes
}

fn jpeg(tag: u8) -> Vec<u8> {
    let mut bytes = jpeg_with_body(32);
    let body_start = 20;
    for byte in &mut bytes[body_start..body_start + 32] {
        *byte = tag;
    }
    bytes
}

fn request(frames: usize) -> UploadRequest {
    UploadRequest::new(
        Rotation::ZERO,
        (0..frames)
            .map(|index| UploadFrame::new(jpeg(index as u8)))
            .collect(),
    )
}

fn headers(plan: &[Operation]) -> Vec<&[u8; DCLD_HEADER_LEN]> {
    plan.iter()
        .filter_map(|operation| match operation {
            Operation::Header(header) => Some(header),
            _ => None,
        })
        .collect()
}

fn chunks_between(plan: &[Operation], header_index: usize) -> Vec<&[u8]> {
    plan.iter()
        .skip(header_index + 1)
        .take_while(|operation| matches!(operation, Operation::Chunk(_)))
        .filter_map(|operation| match operation {
            Operation::Chunk(chunk) => Some(chunk.as_slice()),
            _ => None,
        })
        .collect()
}

#[test]
fn header_is_its_own_operation_and_every_chunk_is_at_most_64_bytes() {
    let jpeg = jpeg_with_body(128); // 150 bytes total: 64 + 64 + 22
    assert_eq!(jpeg.len(), 150);
    let request = UploadRequest::new(Rotation::ZERO, vec![UploadFrame::new(jpeg.clone())]);
    let plan = build_upload_plan(&request).unwrap();

    let header_positions: Vec<usize> = plan
        .iter()
        .enumerate()
        .filter(|(_, operation)| matches!(operation, Operation::Header(_)))
        .map(|(index, _)| index)
        .collect();
    assert_eq!(header_positions.len(), 1);

    let header = headers(&plan)[0];
    assert_eq!(header.len(), 64);
    assert_eq!(&header[..4], b"DCLd");

    let chunks = chunks_between(&plan, header_positions[0]);
    assert_eq!(chunks.len(), 3);
    assert_eq!(
        chunks.iter().map(|chunk| chunk.len()).collect::<Vec<_>>(),
        vec![64, 64, 22]
    );
    assert!(chunks
        .iter()
        .all(|chunk| !chunk.is_empty() && chunk.len() <= IMAGE_CHUNK_LEN));

    let reassembled: Vec<u8> = chunks
        .iter()
        .flat_map(|chunk| chunk.iter().copied())
        .collect();
    assert_eq!(
        reassembled, jpeg,
        "every image byte, in order, exactly once"
    );
}

#[test]
fn a_combined_header_and_jpeg_value_cannot_exist() {
    let plan = build_upload_plan(&request(1)).unwrap();
    // The enum has no Frame variant and Header is a fixed [u8; 64]; this test is
    // mostly a compile-time statement, but it also pins the runtime shape.
    for operation in &plan {
        match operation {
            Operation::Header(header) => assert_eq!(header.len(), DCLD_HEADER_LEN),
            Operation::Chunk(chunk) => assert!(chunk.len() <= IMAGE_CHUNK_LEN),
            Operation::Command(_) | Operation::Finish(_) => {}
        }
    }
    assert!(!plan.iter().any(
        |operation| matches!(operation, Operation::Chunk(chunk) if chunk.len() > IMAGE_CHUNK_LEN)
    ));
}

#[test]
fn every_frame_preserves_all_its_bytes_across_the_whole_plan() {
    let jpegs = [jpeg_with_body(0), jpeg_with_body(1), jpeg_with_body(127)];
    let request = UploadRequest::new(
        Rotation::ZERO,
        jpegs
            .iter()
            .cloned()
            .map(|jpeg| UploadFrame::new(jpeg).with_delay(858))
            .collect(),
    );
    let plan = build_upload_plan(&request).unwrap();

    let mut frames: Vec<(usize, Vec<u8>)> = Vec::new();
    for operation in &plan {
        match operation {
            Operation::Header(header) => {
                frames.push((usize::from(header[17]), Vec::new()));
            }
            Operation::Chunk(chunk) => {
                let (_, bytes) = frames.last_mut().expect("a chunk follows a header");
                bytes.extend_from_slice(chunk);
            }
            _ => {}
        }
    }

    assert_eq!(frames.len(), jpegs.len());
    for (index, (frame_index, bytes)) in frames.iter().enumerate() {
        assert_eq!(*frame_index, index);
        assert_eq!(bytes.as_slice(), jpegs[index].as_slice(), "frame {index}");
    }
}

#[test]
fn an_exact_multiple_of_64_has_no_partial_chunk() {
    let jpeg = jpeg_with_body(106); // 128 bytes
    assert_eq!(jpeg.len(), 128);
    let request = UploadRequest::new(Rotation::ZERO, vec![UploadFrame::new(jpeg)]);
    let plan = build_upload_plan(&request).unwrap();
    let chunks = chunks_between(&plan, 15);
    assert_eq!(
        chunks.iter().map(|chunk| chunk.len()).collect::<Vec<_>>(),
        vec![64, 64]
    );
}

#[test]
fn static_frames_use_count_zero_and_delay_zero() {
    let request = UploadRequest::new(
        Rotation::ZERO,
        vec![UploadFrame::new(jpeg(1)).with_delay(500)],
    );
    let plan = build_upload_plan(&request).unwrap();
    let header = headers(&plan)[0];
    assert_eq!(header[4], DCLD_KIND_STATIC);
    assert_eq!(header[13], 0, "static image: frame_count is 0");
    assert_eq!(header[17], 0, "static image: frame_index is 0");
    assert_eq!(u16::from_le_bytes([header[15], header[16]]), 0);
}

#[test]
fn gif_frames_carry_the_count_index_and_metadata_delay() {
    let request = UploadRequest::new(
        Rotation::ZERO,
        vec![
            UploadFrame::new(jpeg(1)).with_delay(100),
            UploadFrame::new(jpeg(2)).with_delay(200),
            UploadFrame::new(jpeg(3)).with_delay(300),
        ],
    );
    let plan = build_upload_plan(&request).unwrap();
    let headers = headers(&plan);
    assert_eq!(headers.len(), 3);
    for (index, header) in headers.iter().enumerate() {
        assert_eq!(header[4], DCLD_KIND_ANIMATED);
        assert_eq!(header[13], 3);
        assert_eq!(header[17], index as u8);
        assert_eq!(
            u16::from_le_bytes([header[15], header[16]]),
            (index as u16 + 1) * 100
        );
    }
}

#[test]
fn content_id_is_lowercase_hex_in_every_header() {
    let request = request(2);
    let plan = build_upload_plan(&request).unwrap();
    for header in headers(&plan) {
        let id = std::str::from_utf8(&header[20..52]).unwrap();
        assert_eq!(id, request.content_id.as_str());
        assert!(
            id.chars()
                .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase()),
            "{id}"
        );
    }
}

#[test]
fn a_caller_supplied_content_id_reaches_every_header() {
    let id = ContentId::parse("C3BDD15D581D4F6BBC3AEEF13D27A8FB").unwrap();
    let request =
        UploadRequest::new(Rotation::ZERO, vec![UploadFrame::new(jpeg(1))]).with_content_id(id);
    let plan = build_upload_plan(&request).unwrap();
    let header = headers(&plan)[0];
    assert_eq!(&header[20..52], b"c3bdd15d581d4f6bbc3aeef13d27a8fb");
}

#[test]
fn plan_order_matches_the_captured_upload() {
    let plan = build_upload_plan(&request(1)).unwrap();
    let commands: Vec<u8> = plan
        .iter()
        .filter_map(|operation| match operation {
            Operation::Command(command) => Some(command.code()),
            _ => None,
        })
        .collect();

    assert_eq!(
        commands,
        vec![
            // captured preamble: 0x12 … 0x17, 0x0A
            0x12, 0x02, 0x03, 0x04, 0x07, 0x08, 0x05, 0x0B, 0x06, 0x15, 0x16, 0x17, 0x0A,
            // erase, begin
            0x14, 0x0F, // commit
            0x08, // post-commit keepalive
            0x0A, 0x10, 0x12,
        ]
    );

    let begin = plan
        .iter()
        .position(|operation| matches!(operation, Operation::Command(c) if c.code() == 0x0F))
        .unwrap();
    let finish = plan
        .iter()
        .position(|operation| matches!(operation, Operation::Finish(_)))
        .unwrap();
    let commit = plan
            .iter()
            .position(|operation| {
                matches!(operation, Operation::Command(c) if c.code() == 0x08 && c.payload == UPLOAD_COMMIT_PAYLOAD)
            })
            .unwrap();
    assert!(begin < finish && finish < commit);
    assert!(matches!(&plan[begin + 1], Operation::Header(_)));
}

#[test]
fn init_0x08_is_empty_and_the_commit_is_one_byte() {
    let plan = build_upload_plan(&request(1)).unwrap();
    let payloads: Vec<&[u8]> = plan
        .iter()
        .filter_map(|operation| match operation {
            Operation::Command(command) if command.code() == 0x08 => {
                Some(command.payload.as_slice())
            }
            _ => None,
        })
        .collect();
    assert_eq!(payloads, vec![&[][..], &[0x01][..]]);
    assert!(!payloads.contains(&[0x01, 0x01].as_slice()));
    assert!(!payloads.contains(&[0x00, 0x04].as_slice()));
}

#[test]
fn no_plan_contains_the_never_sent_0x11() {
    let plan = build_upload_plan(&request(2)).unwrap();
    assert!(!plan.iter().any(
        |operation| matches!(operation, Operation::Command(command) if command.code() == 0x11)
    ));
}

#[test]
fn labels_and_clock_reach_the_preamble() {
    let clock = Clock::new(11, 19, 10, 11, 46);
    let request = UploadRequest::new(Rotation::ZERO, vec![UploadFrame::new(jpeg(1))])
        .with_labels(MachineLabels::new(
            "Intel Core i9-14900K",
            "NVIDIA GeForce RTX 4070",
        ))
        .with_clock(clock);
    let plan = build_upload_plan(&request).unwrap();
    let command = |code: u8| {
        plan.iter().find_map(|operation| match operation {
            Operation::Command(command) if command.code() == code => Some(&command.payload),
            _ => None,
        })
    };
    assert_eq!(command(0x15).unwrap().as_slice(), b"Intel Core i9-14900K");
    assert_eq!(
        command(0x16).unwrap().as_slice(),
        b"NVIDIA GeForce RTX 4070"
    );
    assert_eq!(command(0x17).unwrap().as_slice(), b"--");
    let clocks: Vec<&Vec<u8>> = plan
        .iter()
        .filter_map(|operation| match operation {
            Operation::Command(command) if command.code() == 0x0A => Some(&command.payload),
            _ => None,
        })
        .collect();
    assert_eq!(clocks.len(), 2);
    // Both 0x0A packets carry the caller's reading, in the clock shape: header,
    // then month, day, hour, minute, second.
    for payload in &clocks {
        assert_eq!(payload.len(), TIME_PAYLOAD_LEN);
        assert_eq!(&payload[..2], &TIME_HEADER);
        assert_eq!(payload[2..], [11, 19, 10, 11, 46]);
    }
    assert_eq!(clocks[0].as_slice(), clock.payload());
    assert_eq!(clocks[1].as_slice(), clock.payload());
}

#[test]
fn every_operation_carries_its_pipe() {
    let plan = build_upload_plan(&request(1)).unwrap();
    for operation in &plan {
        match operation {
            Operation::Command(command) if command.code() == 0x10 => {
                assert_eq!(command.pipe(), Pipe::Data)
            }
            Operation::Command(command) => assert_eq!(command.pipe(), Pipe::Init),
            Operation::Header(_) | Operation::Chunk(_) | Operation::Finish(_) => {
                assert_eq!(operation.pipe(), Pipe::Init)
            }
        }
    }
}

#[test]
fn validate_plan_rejects_oversized_and_empty_chunks() {
    let oversized = Operation::Chunk(vec![0u8; IMAGE_CHUNK_LEN + 1]);
    assert_eq!(
        oversized.validate().unwrap_err(),
        ProtocolError::ImageChunkTooLong {
            actual: IMAGE_CHUNK_LEN + 1,
            max: IMAGE_CHUNK_LEN
        }
    );
    assert_eq!(
        Operation::Chunk(Vec::new()).validate().unwrap_err(),
        ProtocolError::EmptyImageChunk
    );
    assert!(validate_plan(&[oversized]).is_err());
    assert!(validate_plan(&[Operation::Chunk(vec![0u8; IMAGE_CHUNK_LEN])]).is_ok());
}

#[test]
fn rejects_empty_and_oversized_frame_lists() {
    let empty = build_upload_plan(&UploadRequest::new(Rotation::ZERO, vec![])).unwrap_err();
    assert!(matches!(empty, UploadError::NoFrames));

    let too_many = UploadRequest::new(
        Rotation::ZERO,
        (0..256).map(|i| UploadFrame::new(jpeg(i as u8))).collect(),
    );
    assert!(matches!(
        build_upload_plan(&too_many).unwrap_err(),
        UploadError::TooManyFrames { count: 256 }
    ));
}

#[test]
fn rejects_jpegs_that_would_fail_silently() {
    let mut no_jfif = jpeg(1);
    no_jfif[2] = 0x00;
    let request = UploadRequest::new(Rotation::ZERO, vec![UploadFrame::new(no_jfif)]);
    assert!(matches!(
        build_upload_plan(&request).unwrap_err(),
        UploadError::MissingJfif { index: 0 }
    ));

    let request = UploadRequest::new(Rotation::ZERO, vec![UploadFrame::new(vec![0x00, 0x01])]);
    assert!(matches!(
        build_upload_plan(&request).unwrap_err(),
        UploadError::NotJpeg { index: 0 }
    ));

    let request = UploadRequest::new(Rotation::ZERO, vec![UploadFrame::new(vec![])]);
    assert!(matches!(
        build_upload_plan(&request).unwrap_err(),
        UploadError::EmptyFrame { index: 0 }
    ));
}

#[test]
fn preamble_starts_with_device_info_and_ends_with_the_clock() {
    let preamble = upload_preamble(Rotation::new(2).unwrap());
    assert_eq!(preamble.len(), 13);
    assert_eq!(preamble[0].command, Command::DeviceInfo);
    assert_eq!(preamble[1].command, Command::DisplayConfig);
    assert_eq!(preamble[1].payload, [0x01, 0x01, 0x02, 0x00, 0x46]);
    assert_eq!(preamble[9].command, Command::InitStep15);
    assert_eq!(preamble[10].command, Command::InitStep16);
    assert_eq!(preamble[11].command, Command::InitStep17);
    assert_eq!(preamble[12].command, Command::ModeSwitch);
    // No clock source here: the explicit zero reading, never a frozen date.
    assert_eq!(preamble[12].payload.len(), TIME_PAYLOAD_LEN);
    assert_eq!(&preamble[12].payload[..2], &TIME_HEADER);
    assert!(preamble[12].payload[2..].iter().all(|byte| *byte == 0));
    assert!(!preamble.iter().any(|step| step.code() == 0x11));
    assert!(!preamble.iter().any(|step| step.code() == 0x01));
    assert!(!preamble.iter().any(|step| step.code() == 0x10));
}

#[test]
fn every_command_payload_fits_a_packet() {
    let plan = build_upload_plan(&request(2)).unwrap();
    validate_plan(&plan).unwrap();
}

#[test]
fn finish_packet_is_the_captured_terminator() {
    let finish = FinishPacket::new();
    assert_eq!(&finish.bytes()[..10], b"dcldfinish");
    assert_eq!(finish.bytes().len(), 55);
    assert!(finish.bytes()[10..].iter().all(|byte| *byte == 0));
}
