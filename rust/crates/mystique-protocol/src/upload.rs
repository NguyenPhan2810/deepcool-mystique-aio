//! Ordered operation plans for image and GIF upload.
//!
//! # Frame transfer model
//!
//! The 2026-11-18 USBPcap capture of the vendor app on this **360**
//! (`re/captures/usbpcap-upload.pcap`, decoded to `re/captures/upload-decoded.json`)
//! settled the highest-risk question in this protocol:
//!
//! - the 64-byte `DCLd` header is **its own USB transfer**;
//! - the JPEG follows as 64-byte transfers, the last one partial;
//! - `dcldfinish` is a 55-byte transfer of its own.
//!
//! The 240 prior art concatenated header and JPEG into one write. That is what this
//! module used to do, and it is the likely reason the panel showed its placeholder.
//! There is now no type that can hold both parts: [`Operation::Header`] carries exactly
//! 64 bytes and [`Operation::Chunk`] carries at most [`IMAGE_CHUNK_LEN`] bytes. The
//! transport writes each operation as one transfer, and [`validate_plan`] rejects an
//! oversized chunk before it can reach the wire.
//!
//! The wire formats and payloads in this module are from the capture. Timing values and
//! the choice to send the keepalive block once before the erase are inferred, and are
//! marked where they occur.

use std::time::Duration;

use crate::commands::*;
use crate::dcl::{build_finish, build_header, ContentId, DCLD_HEADER_LEN, FINISH_LEN};
use crate::display::DisplayAreas;
use crate::error::ProtocolError;
use crate::init::{machine_info_sequence_with, MachineLabels, SETTLE_MODE_SWITCH, SETTLE_STEP};
use crate::packet::{build, Pipe, MAX_PAYLOAD};
use crate::rotation::Rotation;

/// USB bulk transfer size used for the frame header and every JPEG chunk.
///
/// Observed: 8,961 outbound bulk transfers on endpoint `0x01`, of which 7,359 are
/// exactly 64 bytes (7,345 full JPEG chunks plus all 14 headers). The remaining JPEG
/// chunks are the 14 partial tails. Not one transfer combined a header with image data.
pub const IMAGE_CHUNK_LEN: usize = 64;

/// `0x08` payload that commits an upload. Captured after the `dcldfinish` transfer.
///
/// The init block also sends `0x08`, but with an **empty** payload. The 240 values
/// (`01 01` commit, `00 04` init) are history in [`crate::commands::legacy_240`].
pub const UPLOAD_COMMIT_PAYLOAD: [u8; 1] = [0x01];

/// One JPEG frame plus its display duration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadFrame {
    /// Raw JPEG bytes. Must start with SOI + JFIF APP0; see [`check_jfif`].
    pub jpeg: Vec<u8>,
    /// Per-frame display duration carried in the header's `delay_ms` field. Ignored
    /// for a single-frame (static) upload, where the vendor sends `0` and
    /// `frame_count = 0`.
    pub delay_ms: u16,
}

impl UploadFrame {
    pub fn new(jpeg: Vec<u8>) -> Self {
        Self { jpeg, delay_ms: 0 }
    }

    pub fn with_delay(mut self, delay_ms: u16) -> Self {
        self.delay_ms = delay_ms;
        self
    }
}

/// Validates the two JPEG properties that fail silently on the panel.
///
/// 1. SOI followed **immediately** by a JFIF APP0 segment.
/// 2. Standard Huffman tables — not checkable without a full entropy decode, so it is
///    documented rather than asserted. Encoders that optimise tables must not be used.
pub fn check_jfif(index: usize, jpeg: &[u8]) -> Result<(), UploadError> {
    if jpeg.len() < 2 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
        return Err(UploadError::NotJpeg { index });
    }
    let has_app0 = jpeg.len() >= 11 && jpeg[2] == 0xFF && jpeg[3] == 0xE0;
    let has_jfif = has_app0 && &jpeg[6..11] == b"JFIF\0";
    if !has_jfif {
        return Err(UploadError::MissingJfif { index });
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum UploadError {
    #[error("an upload needs at least one frame")]
    NoFrames,
    #[error("{count} frames exceeds the 8-bit frame counter (maximum 255)")]
    TooManyFrames { count: usize },
    #[error("frame {index} is empty")]
    EmptyFrame { index: usize },
    #[error("frame {index} is not a JPEG (no SOI marker)")]
    NotJpeg { index: usize },
    #[error(
        "frame {index} does not start with SOI + JFIF APP0; the panel rejects such JPEGs silently"
    )]
    MissingJfif { index: usize },
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
}

/// One 48-byte command in a plan.
///
/// The command is a [`Command`], not a bare byte: its pipe and its payload
/// requirement travel with it, so a plan cannot route `0x14` to the data pipe or
/// send an empty `0x0A`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPacket {
    pub command: Command,
    pub payload: Vec<u8>,
    /// How long the driver should wait after the reply before the next step.
    pub settle: Duration,
}

impl CommandPacket {
    pub fn new(command: Command, payload: impl Into<Vec<u8>>, settle: Duration) -> Self {
        Self {
            command,
            payload: payload.into(),
            settle,
        }
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.check_payload()?;
        build(self.command.code(), &self.payload).map(|_| ())
    }

    pub fn packet(&self) -> Result<[u8; crate::packet::PACKET_LEN], ProtocolError> {
        self.check_payload()?;
        build(self.command.code(), &self.payload)
    }

    pub const fn pipe(&self) -> Pipe {
        self.command.pipe()
    }

    pub const fn code(&self) -> u8 {
        self.command.code()
    }

    fn check_payload(&self) -> Result<(), ProtocolError> {
        if self.command.requires_payload() && self.payload.is_empty() {
            return Err(ProtocolError::PayloadRequired {
                command: self.command.code(),
            });
        }
        Ok(())
    }
}

/// The `"dcldfinish"` terminator, 55 bytes: ten ASCII bytes plus 45 zeros.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishPacket([u8; FINISH_LEN]);

impl FinishPacket {
    pub fn new() -> Self {
        Self(build_finish())
    }

    pub fn bytes(&self) -> &[u8; FINISH_LEN] {
        &self.0
    }
}

impl Default for FinishPacket {
    fn default() -> Self {
        Self::new()
    }
}

/// One step of an upload plan.
///
/// A frame is *not* a variant here. It is a [`Self::Header`] followed by zero or more
/// [`Self::Chunk`] values, each one USB transfer. That shape is deliberate: no value
/// of this enum can hold a header and JPEG together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    Command(CommandPacket),
    /// Exactly 64 bytes: the complete `DCLd` header, sent alone.
    Header([u8; DCLD_HEADER_LEN]),
    /// At most [`IMAGE_CHUNK_LEN`] bytes of JPEG data, sent alone.
    Chunk(Vec<u8>),
    Finish(FinishPacket),
}

impl Operation {
    pub fn label(&self) -> String {
        match self {
            Self::Command(command) => format!(
                "0x{:02X} payload {}B",
                command.code(),
                command.payload.len()
            ),
            Self::Header(header) => {
                let index = u16::from(header[17]) + 1;
                let count = header[13];
                if count == 0 {
                    format!("frame {index} static header {DCLD_HEADER_LEN}B")
                } else {
                    format!("frame {index}/{count} header {DCLD_HEADER_LEN}B")
                }
            }
            Self::Chunk(chunk) => format!("image chunk {}B", chunk.len()),
            Self::Finish(_) => "dcldfinish".to_owned(),
        }
    }

    pub const fn settle(&self) -> Duration {
        match self {
            Self::Command(command) => command.settle,
            Self::Header(_) | Self::Chunk(_) | Self::Finish(_) => Duration::ZERO,
        }
    }

    pub const fn pipe(&self) -> Pipe {
        match self {
            // Every control command carries its own pipe. `0x10` in the post-commit
            // keepalive is a data-pipe command; erase, begin-upload and the rest are
            // init-pipe commands.
            Self::Command(command) => command.pipe(),
            // Captured: headers, JPEG chunks and the terminator all go to OUT `0x01`.
            Self::Header(_) | Self::Chunk(_) | Self::Finish(_) => Pipe::Init,
        }
    }

    /// Checks the invariants that keep a plan on the wire.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            Self::Command(command) => command.validate(),
            Self::Header(_) => Ok(()),
            Self::Chunk(chunk) => {
                if chunk.is_empty() {
                    return Err(ProtocolError::EmptyImageChunk);
                }
                if chunk.len() > IMAGE_CHUNK_LEN {
                    return Err(ProtocolError::ImageChunkTooLong {
                        actual: chunk.len(),
                        max: IMAGE_CHUNK_LEN,
                    });
                }
                Ok(())
            }
            Self::Finish(_) => Ok(()),
        }
    }
}

/// Everything needed to build an upload plan.
#[derive(Debug, Clone)]
pub struct UploadRequest {
    pub rotation: Rotation,
    pub frames: Vec<UploadFrame>,
    pub content_id: ContentId,
    /// CPU/GPU text sent by `0x15`/`0x16` in the preamble.
    pub labels: MachineLabels,
    /// Starting value of the `0x0A` counter. The vendor increments it on every send;
    /// its purpose is unknown.
    pub heartbeat: u16,
}

impl UploadRequest {
    /// Derives the content id as an MD5 over the concatenated JPEG bytes.
    ///
    /// The capture shows the vendor's id is a per-upload tag, not a content hash (two
    /// identical static JPEGs got different ids). The device does not validate it, so
    /// a deterministic MD5 is used as a correlation tag; see [`ContentId`].
    pub fn new(rotation: Rotation, frames: Vec<UploadFrame>) -> Self {
        let content_id = ContentId::for_jpegs(frames.iter().map(|frame| frame.jpeg.as_slice()));
        Self {
            rotation,
            frames,
            content_id,
            labels: MachineLabels::default(),
            heartbeat: 0,
        }
    }

    pub fn with_labels(mut self, labels: MachineLabels) -> Self {
        self.labels = labels;
        self
    }

    pub fn with_heartbeat(mut self, heartbeat: u16) -> Self {
        self.heartbeat = heartbeat;
        self
    }

    /// Overrides the deterministic content id. Use [`ContentId::for_upload`] with a
    /// timestamp or random nonce to match the vendor's per-upload id.
    pub fn with_content_id(mut self, content_id: ContentId) -> Self {
        self.content_id = content_id;
        self
    }
}

/// Init steps included in the upload preamble, with default labels and counter 0.
///
/// On the 360 the preamble is simply the same 13-command block the vendor repeats
/// continuously: `0x12` through `0x0A`. It is not the 240's "mode switch first" list.
/// `0x04` carries the default display areas, `[00, 00, 00, 00]`, which is the block the
/// vendor sent during the upload capture.
pub fn upload_preamble(rotation: Rotation) -> Vec<CommandPacket> {
    upload_preamble_with(rotation, &MachineLabels::default(), 0)
}

/// The captured preamble with caller-supplied labels and heartbeat.
pub fn upload_preamble_with(
    rotation: Rotation,
    labels: &MachineLabels,
    heartbeat: u16,
) -> Vec<CommandPacket> {
    machine_info_sequence_with(rotation, labels, heartbeat, DisplayAreas::default())
        .into_iter()
        .map(|step| CommandPacket::new(step.command, step.payload, step.settle))
        .collect()
}

/// Builds the full upload plan.
///
/// The frame count is written into every header, so a plan built from `n` frames can
/// never claim a different total. A single frame is a static image: the vendor sends
/// `frame_count = 0`, `frame_index = 0`, `delay = 0` and header byte 4 `01`.
pub fn build_upload_plan(request: &UploadRequest) -> Result<Vec<Operation>, UploadError> {
    if request.frames.is_empty() {
        return Err(UploadError::NoFrames);
    }
    if request.frames.len() > usize::from(u8::MAX) {
        return Err(UploadError::TooManyFrames {
            count: request.frames.len(),
        });
    }
    for (index, frame) in request.frames.iter().enumerate() {
        if frame.jpeg.is_empty() {
            return Err(UploadError::EmptyFrame { index });
        }
        check_jfif(index, &frame.jpeg)?;
    }

    let animated = request.frames.len() > 1;
    let frame_count = if animated {
        request.frames.len() as u8
    } else {
        0
    };
    let mut plan = Vec::with_capacity(
        32 + request.frames.len()
            + request
                .frames
                .iter()
                .map(|frame| frame.jpeg.len().div_ceil(IMAGE_CHUNK_LEN))
                .sum::<usize>(),
    );

    let push_command =
        |plan: &mut Vec<Operation>, command: CommandPacket| -> Result<(), UploadError> {
            command.validate()?;
            plan.push(Operation::Command(command));
            Ok(())
        };

    // The captured keepalive block: 0x12, 0x02 … 0x17, 0x0A.
    for command in upload_preamble_with(request.rotation, &request.labels, request.heartbeat) {
        push_command(&mut plan, command)?;
    }

    // Erase, then begin. Both carry an empty payload.
    push_command(
        &mut plan,
        CommandPacket::new(Command::EraseImages, [], SETTLE_STEP),
    )?;
    push_command(
        &mut plan,
        CommandPacket::new(Command::BeginUpload, [], SETTLE_STEP),
    )?;

    // One standalone 64-byte header per frame, then the JPEG in 64-byte chunks.
    for (index, frame) in request.frames.iter().enumerate() {
        let delay_ms = if animated { frame.delay_ms } else { 0 };
        plan.push(Operation::Header(build_header(
            &frame.jpeg,
            frame_count,
            delay_ms,
            index as u8,
            &request.content_id,
        )));
        for chunk in frame.jpeg.chunks(IMAGE_CHUNK_LEN) {
            plan.push(Operation::Chunk(chunk.to_vec()));
        }
    }

    // Terminator, then the one-byte commit.
    plan.push(Operation::Finish(FinishPacket::new()));
    push_command(
        &mut plan,
        CommandPacket::new(Command::InitStep08, UPLOAD_COMMIT_PAYLOAD, SETTLE_STEP),
    )?;

    // Captured post-commit keepalive: 0x0A, 0x10, 0x12.
    push_command(
        &mut plan,
        CommandPacket::new(
            Command::ModeSwitch,
            mode_switch_payload(request.heartbeat.wrapping_add(1)),
            SETTLE_MODE_SWITCH,
        ),
    )?;
    push_command(
        &mut plan,
        CommandPacket::new(Command::Status, [], SETTLE_STEP),
    )?;
    push_command(
        &mut plan,
        CommandPacket::new(Command::DeviceInfo, [], SETTLE_STEP),
    )?;

    validate_plan(&plan)?;
    Ok(plan)
}

/// Verifies every operation in a plan is legal before any of it reaches the wire.
///
/// A command payload must fit one 48-byte packet; an image chunk must be non-empty and
/// at most 64 bytes. Because [`Operation::Header`] is a `[u8; 64]`, a header can never
/// carry image data, and no variant can hold a combined header + JPEG buffer.
pub fn validate_plan(plan: &[Operation]) -> Result<(), ProtocolError> {
    for operation in plan {
        if let Operation::Command(command) = operation {
            if command.payload.len() > MAX_PAYLOAD {
                return Err(ProtocolError::PayloadTooLong {
                    actual: command.payload.len(),
                    max: MAX_PAYLOAD,
                });
            }
        }
        operation.validate()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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
        assert!(!plan.iter().any(|operation| matches!(operation, Operation::Chunk(chunk) if chunk.len() > IMAGE_CHUNK_LEN)));
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
                    .all(|character| character.is_ascii_hexdigit()
                        && !character.is_ascii_uppercase()),
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
    fn labels_and_counter_reach_the_preamble() {
        let request = UploadRequest::new(Rotation::ZERO, vec![UploadFrame::new(jpeg(1))])
            .with_labels(MachineLabels::new(
                "Intel Core i9-14900K",
                "NVIDIA GeForce RTX 4070",
            ))
            .with_heartbeat(0x2C3A);
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
        let heartbeats: Vec<&Vec<u8>> = plan
            .iter()
            .filter_map(|operation| match operation {
                Operation::Command(command) if command.code() == 0x0A => Some(&command.payload),
                _ => None,
            })
            .collect();
        assert_eq!(heartbeats.len(), 2);
        assert_eq!(
            heartbeats[0].as_slice(),
            &[0xEA, 0x07, 0x0B, 0x12, 0x0A, 0x2C, 0x3A]
        );
        assert_eq!(
            heartbeats[1].as_slice(),
            &[0xEA, 0x07, 0x0B, 0x12, 0x0A, 0x2C, 0x3B]
        );
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
    fn preamble_starts_with_device_info_and_ends_with_the_heartbeat() {
        let preamble = upload_preamble(Rotation::new(2).unwrap());
        assert_eq!(preamble.len(), 13);
        assert_eq!(preamble[0].command, Command::DeviceInfo);
        assert_eq!(preamble[1].command, Command::DisplayConfig);
        assert_eq!(preamble[1].payload, [0x01, 0x01, 0x02, 0x00, 0x46]);
        assert_eq!(preamble[9].command, Command::InitStep15);
        assert_eq!(preamble[10].command, Command::InitStep16);
        assert_eq!(preamble[11].command, Command::InitStep17);
        assert_eq!(preamble[12].command, Command::ModeSwitch);
        assert_eq!(
            preamble[12].payload,
            vec![0xEA, 0x07, 0x0B, 0x12, 0x0A, 0x00, 0x00]
        );
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
}
