//! The raw packet surface. It exists so the owner can exercise any command byte
//! without waiting for a typed command. The daemon builds the 48-byte packet, so the
//! UI never computes a checksum, and the result code is reported as data rather than
//! gating the exchange.

use std::time::Instant;

use mystique_ipc::{args, hex, CommandError, PacketRecord, RawPacketOutcome, Request, Response};
use mystique_protocol::{build_packet, Pipe};

use super::{failed, ok, parse};
use crate::core::Core;
use crate::error;

impl Core {
    pub(super) fn command_raw_packet(&mut self, request: &Request, now: Instant) -> Response {
        let args = match parse::<args::RawPacket>(request) {
            Ok(value) => value,
            Err(error) => return failed(request, error),
        };
        let pipe = match parse_pipe(&args.pipe) {
            Ok(pipe) => pipe,
            Err(error) => return failed(request, error),
        };
        let packet = match build_packet(args.command, &args.payload) {
            Ok(packet) => packet,
            Err(error) => return failed(request, error::from_protocol(error)),
        };
        let packet_hex = hex(&packet);
        let payload_hex = hex(&args.payload);

        if args.dry_run {
            return ok(
                request,
                &RawPacketOutcome {
                    command: args.command,
                    pipe: pipe.label().to_owned(),
                    payload_hex,
                    packet_hex,
                    dry_run: true,
                    sent: false,
                    result: None,
                    result_code: None,
                    reply_hex: None,
                    record: None,
                },
            );
        }

        let Some(panel) = self.panel.as_mut() else {
            return failed(request, CommandError::not_open());
        };

        let mut record = PacketRecord {
            label: format!("raw 0x{:02X}", args.command),
            code: Some(args.command),
            pipe: pipe.label().to_owned(),
            payload_hex,
            packet_hex,
            result: "not sent".to_owned(),
            reply_hex: None,
        };

        match panel.send_raw(pipe, args.command, &args.payload) {
            Ok(response) => {
                let result = response.result();
                let reply_hex = response.redacted_hex();
                record.result = result.to_string();
                record.reply_hex = Some(reply_hex.clone());
                self.last_error = None;
                ok(
                    request,
                    &RawPacketOutcome {
                        command: args.command,
                        pipe: pipe.label().to_owned(),
                        payload_hex: record.payload_hex.clone(),
                        packet_hex: record.packet_hex.clone(),
                        dry_run: false,
                        sent: true,
                        result: Some(result.to_string()),
                        result_code: Some(result.as_raw()),
                        reply_hex: Some(reply_hex),
                        record: Some(record),
                    },
                )
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
                failed(request, error)
            }
        }
    }
}

fn parse_pipe(text: &str) -> Result<Pipe, CommandError> {
    match text {
        "init" => Ok(Pipe::Init),
        "data" => Ok(Pipe::Data),
        other => Err(CommandError::range(format!(
            "pipe must be \"init\" or \"data\", got \"{other}\""
        ))),
    }
}
