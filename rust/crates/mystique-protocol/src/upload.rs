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

use crate::clock::Clock;
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
    /// The local time carried by `0x0A` in the preamble and the post-commit keepalive.
    ///
    /// Both packets use this one reading: the plan is built from a single clock value and
    /// the periodic `0x0A` corrects the panel a second later. [`Clock::zero`] means the
    /// caller has no clock source; it is never a frozen real date.
    pub clock: Clock,
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
            clock: Clock::zero(),
        }
    }

    pub fn with_labels(mut self, labels: MachineLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Sets the `0x0A` reading. The daemon passes the live local time; tests and dry runs
    /// leave the [`Clock::zero`] default.
    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    /// Overrides the deterministic content id. Use [`ContentId::for_upload`] with a
    /// timestamp or random nonce to match the vendor's per-upload id.
    pub fn with_content_id(mut self, content_id: ContentId) -> Self {
        self.content_id = content_id;
        self
    }
}

/// Init steps included in the upload preamble, with default labels and [`Clock::zero`].
///
/// On the 360 the preamble is simply the same 13-command block the vendor repeats
/// continuously: `0x12` through `0x0A`. It is not the 240's "mode switch first" list.
/// `0x04` carries the default display areas, `[00, 00, 00, 00]`, which is the block the
/// vendor sent during the upload capture. Use [`upload_preamble_with`] for labels and a
/// real clock.
pub fn upload_preamble(rotation: Rotation) -> Vec<CommandPacket> {
    upload_preamble_with(rotation, &MachineLabels::default(), Clock::zero())
}

/// The captured preamble with caller-supplied labels and the clock reading for `0x0A`.
pub fn upload_preamble_with(
    rotation: Rotation,
    labels: &MachineLabels,
    clock: Clock,
) -> Vec<CommandPacket> {
    machine_info_sequence_with(rotation, labels, clock, DisplayAreas::default())
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
    for command in upload_preamble_with(request.rotation, &request.labels, request.clock) {
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

    // Captured post-commit keepalive: 0x0A, 0x10, 0x12. The clock is the same reading the
    // preamble used; a plan is built from one value and the periodic 0x0A updates it.
    push_command(
        &mut plan,
        CommandPacket::new(
            Command::ModeSwitch,
            request.clock.payload(),
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
#[cfg(test)]
mod tests;
