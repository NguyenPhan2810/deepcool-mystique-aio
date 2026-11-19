//! Upload planning and execution on top of the protocol crate's ordered plan.
//!
//! The plan is the captured vendor sequence: the 13-command preamble, erase, begin, a
//! 64-byte `DCLd` header per frame, 64-byte JPEG chunks, the `dcldfinish` terminator,
//! the commit and the post-commit keepalive. The preview and the upload share one
//! frame set and content id, so the counts the owner confirms are the counts sent.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use mystique_ipc::{
    codes, hex, CommandError, PacketRecord, PlanCommandSummary, UploadKind, UploadPlanSummary,
};
use mystique_media::{FitMode, LoadedMedia, PANEL_HEIGHT, PANEL_WIDTH};
use mystique_protocol::{
    build_upload_plan, Clock, ContentId, Operation, UploadFrame, UploadRequest,
};

use crate::error;
use crate::panel::Panel;
use crate::state::Settings;

/// A preview the owner confirmed. Reused when the path, kind and fit still match.
pub struct PendingUpload {
    pub path: PathBuf,
    pub kind: UploadKind,
    pub media: LoadedMedia,
    pub content_id: ContentId,
}

pub struct ExecutionReport {
    pub packets: Vec<PacketRecord>,
    pub chunks: usize,
    pub chunk_bytes: usize,
}

/// Reads the six-byte magic only; decoding is left to `mystique-media`.
pub fn sniff_kind(path: &Path) -> Result<UploadKind, CommandError> {
    let mut file = std::fs::File::open(path).map_err(|source| error::read_error(path, source))?;
    let mut magic = [0u8; 6];
    let read = file
        .read(&mut magic)
        .map_err(|source| error::read_error(path, source))?;
    if read == magic.len() && (&magic == b"GIF87a" || &magic == b"GIF89a") {
        Ok(UploadKind::Gif)
    } else {
        Ok(UploadKind::Image)
    }
}

/// `fit` absent means [`FitMode::Cover`], the panel-filling default. An unknown word is
/// a range error, never a silent fallback.
pub fn parse_fit(fit: Option<&str>) -> Result<FitMode, CommandError> {
    match fit {
        None => Ok(FitMode::default()),
        Some(text) => FitMode::parse(text).ok_or_else(|| {
            CommandError::range(format!("unknown fit mode \"{text}\"; use cover or contain"))
                .with_hint("cover fills the panel and crops; contain letterboxes")
        }),
    }
}

pub fn load_frames(path: &Path, fit: FitMode) -> Result<LoadedMedia, CommandError> {
    mystique_media::load_with(path, fit).map_err(error::from_media)
}

/// A fresh content id per upload. The vendor uses a per-upload tag, not a content hash.
pub fn fresh_content_id(frames: &[UploadFrame]) -> ContentId {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_le_bytes();
    ContentId::for_upload(frames.iter().map(|frame| frame.jpeg.as_slice()), &nonce)
}

/// Builds the plan and the summary from one frame set and the clock reading the `0x0A`
/// packets will carry.
pub fn make_plan(
    settings: &Settings,
    path: &Path,
    media: &LoadedMedia,
    content_id: ContentId,
    clock: Clock,
) -> Result<(Vec<Operation>, UploadPlanSummary), CommandError> {
    let rotation = settings.display.rotation();
    let frames = media.frames.clone();
    let request = UploadRequest::new(rotation, frames)
        .with_labels(settings.labels.clone())
        .with_clock(clock)
        .with_content_id(content_id);
    let plan = build_upload_plan(&request).map_err(|error| {
        CommandError::new(codes::UPLOAD, error.to_string())
            .with_hint("the file must decode to at least one JFIF JPEG frame")
    })?;

    let mut total_bytes = 0usize;
    let mut chunk_count = 0usize;
    let mut header_count = 0usize;
    let mut command_count = 0usize;
    let mut commands = Vec::new();

    for operation in &plan {
        match operation {
            Operation::Command(packet) => {
                command_count += 1;
                let bytes = packet.packet().map_err(error::from_protocol)?;
                commands.push(PlanCommandSummary {
                    label: error::command_label(packet.command),
                    pipe: packet.pipe().label().to_owned(),
                    payload_hex: hex(&packet.payload),
                    packet_hex: hex(&bytes),
                });
            }
            Operation::Header(_) => header_count += 1,
            Operation::Chunk(chunk) => {
                chunk_count += 1;
                total_bytes += chunk.len();
            }
            Operation::Finish(_) => {}
        }
    }

    let duration_ms = request
        .frames
        .iter()
        .map(|frame| u64::from(frame.delay_ms))
        .sum();
    let summary = UploadPlanSummary {
        path: path.display().to_string(),
        kind: if request.frames.len() > 1 {
            UploadKind::Gif
        } else {
            UploadKind::Image
        },
        fit: media.fit.label().to_owned(),
        source_width: media.source_width,
        source_height: media.source_height,
        output_width: PANEL_WIDTH,
        output_height: PANEL_HEIGHT,
        frames: request.frames.len(),
        total_bytes,
        chunk_count,
        header_count,
        command_count,
        operations: plan.len(),
        rotation: rotation.raw(),
        duration_ms,
        content_id: content_id.to_string(),
        preview: request
            .frames
            .first()
            .and_then(mystique_media::preview_data_url),
        commands,
    };
    Ok((plan, summary))
}

/// Executes every operation, collecting one log record per command, frame header and
/// terminator. Image chunks are counted, not logged individually.
pub fn execute_plan(
    panel: &mut dyn Panel,
    plan: &[Operation],
) -> Result<ExecutionReport, CommandError> {
    let mut packets = Vec::new();
    let mut chunks = 0usize;
    let mut chunk_bytes = 0usize;

    for operation in plan {
        match operation {
            Operation::Chunk(chunk) => {
                if let Err(failure) = panel.execute(operation) {
                    return Err(error::from_device(failure).with_packets(packets));
                }
                chunks += 1;
                chunk_bytes += chunk.len();
            }
            Operation::Command(command) => {
                let mut record = error::packet_record(command)?;
                match panel.execute(operation) {
                    Ok(Some(response)) => {
                        record.result = response.result().to_string();
                        record.reply_hex = Some(response.redacted_hex());
                        packets.push(record);
                    }
                    Ok(None) => {
                        record.result = "sent (no reply)".to_owned();
                        packets.push(record);
                    }
                    Err(failure) => {
                        record.result = format!("error: {failure}");
                        packets.push(record);
                        return Err(error::from_device(failure).with_packets(packets));
                    }
                }
            }
            Operation::Header(header) => {
                let mut record = error::raw_record(operation.label(), hex(header), "init");
                if let Err(failure) = panel.execute(operation) {
                    record.result = format!("error: {failure}");
                    packets.push(record);
                    return Err(error::from_device(failure).with_packets(packets));
                }
                record.result = "sent (no reply)".to_owned();
                packets.push(record);
            }
            Operation::Finish(finish) => {
                let mut record = error::raw_record(operation.label(), hex(finish.bytes()), "init");
                if let Err(failure) = panel.execute(operation) {
                    record.result = format!("error: {failure}");
                    packets.push(record);
                    return Err(error::from_device(failure).with_packets(packets));
                }
                record.result = "sent (no reply)".to_owned();
                packets.push(record);
            }
        }
    }

    Ok(ExecutionReport {
        packets,
        chunks,
        chunk_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg() -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        bytes.extend_from_slice(b"JFIF\0");
        bytes.extend_from_slice(&[0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00]);
        bytes.extend(std::iter::repeat_n(0x42, 100));
        bytes.extend_from_slice(&[0xFF, 0xD9]);
        bytes
    }

    fn sample_media(frames: Vec<UploadFrame>) -> LoadedMedia {
        LoadedMedia {
            frames,
            source_width: 1920,
            source_height: 1080,
            fit: FitMode::Cover,
        }
    }

    #[test]
    fn the_plan_summary_matches_the_protocol_plan() {
        let settings = Settings::default();
        let frames = vec![UploadFrame::new(jpeg())];
        let content_id = ContentId::for_jpegs(frames.iter().map(|frame| frame.jpeg.as_slice()));
        let media = sample_media(frames);
        let (plan, summary) = make_plan(
            &settings,
            Path::new("panel.png"),
            &media,
            content_id,
            Clock::zero(),
        )
        .unwrap();

        assert_eq!(summary.frames, 1);
        assert_eq!(summary.kind, UploadKind::Image);
        assert_eq!(summary.fit, "cover");
        assert_eq!(summary.output_width, 480);
        assert_eq!(summary.output_height, 640);
        assert_eq!(summary.total_bytes, 122);
        assert_eq!(summary.chunk_count, 2);
        assert_eq!(summary.header_count, 1);
        assert_eq!(summary.command_count, 19);
        assert_eq!(summary.operations, plan.len());
        assert_eq!(summary.commands.len(), 19);
        assert!(summary
            .commands
            .iter()
            .all(|command| command.packet_hex.starts_with("AA 2E ")));
        assert!(
            summary.preview.is_none(),
            "this synthetic JPEG is not a PNG"
        );
    }

    #[test]
    fn gif_frames_are_counted_with_their_delays() {
        let settings = Settings::default();
        let frames = vec![
            UploadFrame::new(jpeg()).with_delay(80),
            UploadFrame::new(jpeg()).with_delay(120),
        ];
        let content_id = ContentId::for_jpegs(frames.iter().map(|frame| frame.jpeg.as_slice()));
        let (_plan, summary) = make_plan(
            &settings,
            Path::new("anim.gif"),
            &sample_media(frames),
            content_id,
            Clock::zero(),
        )
        .unwrap();
        assert_eq!(summary.kind, UploadKind::Gif);
        assert_eq!(summary.frames, 2);
        assert_eq!(summary.header_count, 2);
        assert_eq!(summary.duration_ms, 200);
    }

    #[test]
    fn the_fit_argument_defaults_to_cover_and_rejects_typos() {
        assert_eq!(parse_fit(None).unwrap(), FitMode::Cover);
        assert_eq!(parse_fit(Some("cover")).unwrap(), FitMode::Cover);
        assert_eq!(parse_fit(Some("Contain")).unwrap(), FitMode::Contain);
        let error = parse_fit(Some("stretch")).unwrap_err();
        assert_eq!(error.code, "range");
        assert!(error.message.contains("stretch"));
    }

    #[test]
    fn sniffing_reads_the_gif_magic() {
        let dir = std::env::temp_dir().join(format!("mystique-daemon-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let gif = dir.join("magic.gif");
        std::fs::write(&gif, b"GIF89a\x01\x00\x01\x00").unwrap();
        assert_eq!(sniff_kind(&gif).unwrap(), UploadKind::Gif);

        let png = dir.join("magic.png");
        std::fs::write(&png, b"\x89PNG\r\n\x1a\n").unwrap();
        assert_eq!(sniff_kind(&png).unwrap(), UploadKind::Image);

        let short = dir.join("short.bin");
        std::fs::write(&short, b"GIF").unwrap();
        assert_eq!(sniff_kind(&short).unwrap(), UploadKind::Image);
    }
}
