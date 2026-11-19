//! The control API, one function per Tauri command name.
//!
//! Argument names and return DTOs mirror `apps/mystique-ui`; `set_screen` is the one
//! deliberate rename (the UI's editor calls it `set_screen_power`). Screen off is
//! brightness 0, with the last lit brightness remembered. `shutdown` is not here: it
//! concerns the process, not the device session, and is answered by the connection
//! handler before the worker is stopped.
//!
//! Every handler returns a [`Response`] rather than a `Result`, so one unknown command
//! or bad argument can never take the connection down. A device fault additionally
//! drops the handle and schedules a reconnect; a range error does not.
//!
//! `detected_labels` is the one command not here: it is a blocking host probe with no
//! device session, answered by the connection handler in `client.rs` so it cannot stall
//! the telemetry loop.

mod raw;
mod settings;
mod upload;

use std::time::{Duration, Instant};

use mystique_ipc::{
    args, cmd, codes, hex, CommandError, Event, OpenResult, PacketRecord, Request, Response,
    StatusDto, UploadKind,
};
use mystique_protocol::{Command, CommandPacket};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::core::Core;
use crate::error;

impl Core {
    /// Dispatches one request and returns the response plus the state event that should
    /// be broadcast after it.
    pub fn handle(&mut self, request: &Request, now: Instant) -> (Response, Vec<Event>) {
        let response = match request.cmd.as_str() {
            cmd::STATUS => self.command_status(request, now),
            cmd::DEVICE_STATE => self.command_device_state(request),
            cmd::OPEN_DEVICE => self.command_open(request, now),
            cmd::CLOSE_DEVICE => self.command_close(request),
            cmd::SET_BRIGHTNESS => self.command_set_brightness(request, now),
            cmd::SET_ROTATION => self.command_set_rotation(request, now),
            cmd::SET_GYRO => self.command_set_gyro(request, now),
            cmd::SET_SCREEN => self.command_set_screen(request, now),
            cmd::SET_IDLE_SCREEN => self.command_set_idle_screen(request, now),
            cmd::SET_RGB_SYNC => self.command_set_rgb_sync(request, now),
            cmd::SET_RGB_MODE => self.command_set_rgb_mode(request, now),
            cmd::SET_RECORDING => self.command_set_recording(request, now),
            cmd::SET_MODE => self.command_set_mode(request, now),
            cmd::SET_FAN_IO => self.command_set_fan_io(request, now),
            cmd::SET_AREAS => self.command_set_areas(request, now),
            cmd::SET_MEDIA_TIMING => self.command_set_media_timing(request, now),
            cmd::SET_LABELS => self.command_set_labels(request, now),
            cmd::RAW_PACKET => self.command_raw_packet(request, now),
            cmd::ERASE_IMAGES => self.command_erase_images(request, now),
            cmd::PLAN_UPLOAD => self.command_plan_upload(request),
            cmd::UPLOAD_IMAGE => self.command_upload(request, now, UploadKind::Image),
            cmd::UPLOAD_GIF => self.command_upload(request, now, UploadKind::Gif),
            cmd::START_STREAM => self.command_start_stream(request, now),
            cmd::STOP_STREAM => self.command_stop_stream(request),
            cmd::SET_STREAM_RATE => self.command_set_stream_rate(request, now),
            other => failed(request, unknown_command(other)),
        };
        // A settings command that succeeded changed the session; write the whole file.
        // A failed command changed nothing, and the device error is already the answer.
        if response.ok && crate::settings_file::persists(&request.cmd) {
            self.persist_settings();
        }
        (response, vec![self.state_event()])
    }

    fn command_status(&mut self, request: &Request, now: Instant) -> Response {
        self.refresh_environment();
        match self.live_status(now) {
            Ok(status) => ok(request, &status),
            Err(error) => failed(request, error),
        }
    }

    fn command_device_state(&mut self, request: &Request) -> Response {
        self.refresh_environment();
        ok(request, &self.state())
    }

    fn command_open(&mut self, request: &Request, now: Instant) -> Response {
        if self.panel.is_some() {
            return ok(
                request,
                &OpenResult {
                    already_open: true,
                    message: "the device is already open in this daemon".to_owned(),
                    device: None,
                    endpoints_ok: true,
                },
            );
        }
        self.auto_reconnect = true;
        match self.open_once() {
            Ok(()) => {
                self.settings.last_rotation = None;
                self.note_open_success(now);
                let info = self.panel.as_ref().and_then(|panel| panel.info());
                let endpoints_ok = self
                    .panel
                    .as_ref()
                    .is_some_and(|panel| panel.endpoints_ok());
                ok(
                    request,
                    &OpenResult {
                        already_open: false,
                        message: "device opened; this daemon now owns the USB interface".to_owned(),
                        device: info,
                        endpoints_ok,
                    },
                )
            }
            Err(error) => {
                self.note_open_failure(&error, now);
                failed(request, error)
            }
        }
    }

    fn command_close(&mut self, request: &Request) -> Response {
        self.panel = None;
        self.pending_upload = None;
        self.stream.stop();
        self.auto_reconnect = false;
        self.last_error = None;
        ok(request, &self.state())
    }

    fn command_start_stream(&mut self, request: &Request, now: Instant) -> Response {
        self.stream.start();
        self.note_stream_started(now);
        ok(request, &self.telemetry_status())
    }

    fn command_stop_stream(&mut self, request: &Request) -> Response {
        self.stream.stop();
        ok(request, &self.telemetry_status())
    }

    /// Changes the period of a running stream. No device command: the rate is a host
    /// property, not something the panel can be told.
    fn command_set_stream_rate(&mut self, request: &Request, now: Instant) -> Response {
        let args = match parse::<args::StreamRate>(request) {
            Ok(value) => value,
            Err(error) => return failed(request, error),
        };
        if !(1..=120).contains(&args.hz) {
            return failed(
                request,
                CommandError::range(format!(
                    "stream rate {} Hz is outside the supported 1..=120",
                    args.hz
                )),
            );
        }
        let interval = Duration::from_secs_f64(1.0 / f64::from(args.hz));
        self.set_stream_interval(interval, now);
        ok(request, &self.telemetry_status())
    }

    /// `0x10`, advisory, only when the device is open. Mirrors the UI's `status`.
    fn live_status(&mut self, now: Instant) -> Result<StatusDto, CommandError> {
        let mut device = self.state();
        let mut rotation_source = "unknown".to_owned();
        let mut rotation_degrees = degrees(device.rotation);
        if device.rotation.is_some() {
            rotation_source = "session (last set)".to_owned();
        }
        let mut mode = None;
        let mut mode_raw = None;
        let mut result = None;
        let mut packet_hex = None;

        if self.panel.is_none() {
            return Ok(StatusDto {
                device,
                rotation_source,
                rotation_degrees,
                mode,
                mode_raw,
                result,
                packet_hex,
            });
        }

        let request = CommandPacket::new(Command::Status, Vec::new(), Duration::ZERO);
        let request_hex = hex(&request.packet().map_err(error::from_protocol)?);
        let live = match self.panel.as_mut() {
            Some(panel) => panel.status(),
            None => return Err(CommandError::not_open()),
        };
        match live {
            Ok(status) => {
                device.rotation = Some(status.rotation);
                self.settings.last_rotation = Some(status.rotation);
                rotation_source = "0x10 reply (advisory)".to_owned();
                rotation_degrees = status.rotation_degrees();
                mode = Some(status.mode.label().to_owned());
                mode_raw = Some(status.raw_mode);
                result = Some(status.result.to_string());
                packet_hex = Some(request_hex);
                Ok(StatusDto {
                    device,
                    rotation_source,
                    rotation_degrees,
                    mode,
                    mode_raw,
                    result,
                    packet_hex,
                })
            }
            Err(failure) => {
                let record = PacketRecord {
                    label: error::command_label(Command::Status),
                    code: Some(Command::Status.code()),
                    pipe: Command::Status.pipe().label().to_owned(),
                    payload_hex: String::new(),
                    packet_hex: request_hex,
                    result: format!("error: {failure}"),
                    reply_hex: None,
                };
                let error = error::from_device(failure).with_packets(vec![record]);
                self.last_error = Some(error.message.clone());
                if error::should_reacquire(&error.code) {
                    self.stream.note_error(error.message.clone());
                    self.panel = None;
                    self.schedule_retry(now, &error.code);
                }
                Err(error)
            }
        }
    }

    /// Writes one packet, recording it before the write. A transport failure drops the
    /// handle and schedules the reconnect before the error is returned; a rejection
    /// keeps the handle, because the device answered.
    fn write_packet(
        &mut self,
        packet: CommandPacket,
        now: Instant,
    ) -> Result<PacketRecord, CommandError> {
        let mut record = error::packet_record(&packet)?;
        let sent = match self.panel.as_mut() {
            Some(panel) => panel.send(packet.command, &packet.payload),
            None => return Err(CommandError::not_open()),
        };
        match sent {
            Ok(response) => {
                record.result = response.result().to_string();
                record.reply_hex = Some(response.redacted_hex());
                self.last_error = None;
                Ok(record)
            }
            Err(failure) => {
                record.result = format!("error: {failure}");
                let error = error::from_device(failure).with_packets(vec![record]);
                self.last_error = Some(error.message.clone());
                if error::should_reacquire(&error.code) {
                    self.panel = None;
                    self.stream.note_error(error.message.clone());
                    self.schedule_retry(now, &error.code);
                }
                Err(error)
            }
        }
    }

    fn write_packets(
        &mut self,
        packets: Vec<CommandPacket>,
        now: Instant,
    ) -> Result<Vec<PacketRecord>, CommandError> {
        let mut records = Vec::with_capacity(packets.len());
        for packet in packets {
            match self.write_packet(packet, now) {
                Ok(record) => records.push(record),
                Err(mut error) => {
                    let mut all = records;
                    all.append(&mut error.packets);
                    return Err(error.with_packets(all));
                }
            }
        }
        Ok(records)
    }

    fn outcome(&self, packets: Vec<PacketRecord>, message: String) -> mystique_ipc::CommandOutcome {
        mystique_ipc::CommandOutcome {
            packets,
            settings: self.settings.snapshot(),
            message,
        }
    }
}

fn ok<T: Serialize>(request: &Request, data: &T) -> Response {
    Response::success(request.id, data)
        .unwrap_or_else(|error| failed(request, CommandError::internal(error.to_string())))
}

fn failed(request: &Request, error: CommandError) -> Response {
    Response::failure(request.id, error)
}

fn parse<T: DeserializeOwned>(request: &Request) -> Result<T, CommandError> {
    request.parse_args::<T>().map_err(|error| {
        CommandError::range(error.to_string())
            .with_hint("check the argument names and types for this command")
    })
}

fn unknown_command(name: &str) -> CommandError {
    CommandError::new(
        codes::UNKNOWN_COMMAND,
        format!("unknown command \"{name}\""),
    )
    .with_hint(format!("known commands: {}", cmd::ALL.join(", ")))
}

fn degrees(raw: Option<u8>) -> Option<u16> {
    raw.filter(|value| *value <= 3)
        .map(|value| u16::from(value) * 90)
}
