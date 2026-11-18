//! Upload preview and send. The plan itself lives in [`crate::upload`]; these handlers
//! only wire it to the session and the device.

use std::path::Path;
use std::time::Instant;

use mystique_ipc::{args, cmd, codes, CommandError, Request, Response, UploadKind};

use super::{failed, ok, parse};
use crate::core::Core;
use crate::error;
use crate::upload::{self, PendingUpload};

impl Core {
    pub(super) fn command_plan_upload(&mut self, request: &Request) -> Response {
        let args = match parse::<args::Upload>(request) {
            Ok(value) => value,
            Err(error) => return failed(request, error),
        };
        let fit = match upload::parse_fit(args.fit.as_deref()) {
            Ok(value) => value,
            Err(error) => return failed(request, error),
        };
        let path = Path::new(&args.path);
        let kind = match upload::sniff_kind(path) {
            Ok(value) => value,
            Err(error) => return failed(request, error),
        };
        let media = match upload::load_frames(path, fit) {
            Ok(value) => value,
            Err(error) => return failed(request, error),
        };
        let content_id = upload::fresh_content_id(&media.frames);
        let (_plan, summary) = match upload::make_plan(&self.settings, path, &media, content_id) {
            Ok(value) => value,
            Err(error) => return failed(request, error),
        };
        self.pending_upload = Some(PendingUpload {
            path: path.to_path_buf(),
            kind,
            media,
            content_id,
        });
        ok(request, &summary)
    }

    pub(super) fn command_upload(
        &mut self,
        request: &Request,
        now: Instant,
        expected: UploadKind,
    ) -> Response {
        if self.panel.is_none() {
            return failed(request, CommandError::not_open());
        }
        let args = match parse::<args::Upload>(request) {
            Ok(value) => value,
            Err(error) => return failed(request, error),
        };
        let fit = match upload::parse_fit(args.fit.as_deref()) {
            Ok(value) => value,
            Err(error) => return failed(request, error),
        };
        let path = Path::new(&args.path);
        let kind = match upload::sniff_kind(path) {
            Ok(value) => value,
            Err(error) => return failed(request, error),
        };
        if kind != expected {
            let right = if kind == UploadKind::Gif {
                cmd::UPLOAD_GIF
            } else {
                cmd::UPLOAD_IMAGE
            };
            return failed(
                request,
                CommandError::new(
                    codes::WRONG_KIND,
                    format!(
                        "{} looks like a {}; use {right} instead",
                        path.display(),
                        kind.label()
                    ),
                ),
            );
        }

        // Reuse the preview's frames and content id when path, kind and fit all match:
        // the counts the owner confirmed are the counts that get sent.
        let cached = match self.pending_upload.take() {
            Some(pending)
                if pending.path.as_path() == path
                    && pending.kind == kind
                    && pending.media.fit == fit =>
            {
                Some(pending)
            }
            Some(pending) => {
                self.pending_upload = Some(pending);
                None
            }
            None => None,
        };
        let (media, content_id) = match cached {
            Some(pending) => (pending.media, pending.content_id),
            None => {
                let media = match upload::load_frames(path, fit) {
                    Ok(value) => value,
                    Err(error) => return failed(request, error),
                };
                let content_id = upload::fresh_content_id(&media.frames);
                (media, content_id)
            }
        };
        let (plan, summary) = match upload::make_plan(&self.settings, path, &media, content_id) {
            Ok(value) => value,
            Err(error) => return failed(request, error),
        };

        // An upload enters image mode, and a failed attempt can leave the panel there.
        // Stop the stream before the first transfer so a 0x01 frame never follows a
        // partial upload; Start stream is an explicit decision after that.
        self.stream.stop();

        let report = match self.panel.as_mut() {
            Some(panel) => upload::execute_plan(panel.as_mut(), &plan),
            None => return failed(request, CommandError::not_open()),
        };
        match report {
            Ok(report) => {
                self.last_error = None;
                let message = format!(
                    "uploaded {} frame{}: {} bytes in {} chunks, {} command packets \
                     ({} to {}x{}). The panel is in image mode; the telemetry stream is stopped.",
                    summary.frames,
                    if summary.frames == 1 { "" } else { "s" },
                    report.chunk_bytes,
                    report.chunks,
                    summary.command_count,
                    summary.fit,
                    summary.output_width,
                    summary.output_height
                );
                ok(request, &self.outcome(report.packets, message))
            }
            Err(error) => {
                self.last_error = Some(error.message.clone());
                if error::should_reacquire(&error.code) {
                    self.panel = None;
                    self.stream.note_error(error.message.clone());
                    self.schedule_retry(now, &error.code);
                }
                failed(request, error)
            }
        }
    }
}
