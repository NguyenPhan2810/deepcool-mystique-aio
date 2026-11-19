//! Message types and newline-delimited JSON framing for the `mystique-daemon` control
//! socket.
//!
//! This crate contains **no I/O and no transport policy**. It defines what a message
//! looks like, how it is encoded as one line of JSON, and how a byte stream is split
//! into messages. The daemon owns the socket (a Windows named pipe or a Unix socket);
//! a client owns its end. Both share these types, so a UI can re-point at the daemon
//! without redefining a single DTO.
//!
//! # Wire model
//!
//! One message per line, UTF-8 JSON, `\n` terminated. Clients send
//! [`Request`] values; the daemon answers each one with a [`Response`] carrying the
//! same `id`. Unsolicited [`Event`] values are broadcast to every connected client.
//!
//! ```text
//! -> {"id":1,"cmd":"set_brightness","args":{"brightness":40}}
//! <- {"id":1,"ok":true,"data":{"packets":[],"settings":{...},"message":"..."}}
//! <- {"event":"telemetry","frame":{...}}
//! <- {"event":"state","state":{...}}
//! ```
//!
//! The DTOs mirror the existing Tauri command surface (`mystique-ui`) field for field.
//! Where the daemon knows something the desktop app did not, the extra field is
//! optional, so a Tauri-shaped client deserialises the payload unchanged.
//!
//! # Framing
//!
//! [`LineDecoder`] buffers bytes and yields complete messages, so a read may split a
//! message anywhere, including inside a multi-byte UTF-8 sequence. Blank lines are
//! ignored and a trailing `\r` is accepted. A line that never ends is rejected once it
//! passes [`DEFAULT_MAX_LINE`], so a misbehaving client cannot grow the buffer without
//! bound.

#![forbid(unsafe_code)]

pub mod dto;
pub mod error;
pub mod event;
pub mod frame;
pub mod message;
pub mod request;
pub mod response;

pub use dto::{
    CommandOutcome, DetectedLabels, DeviceInfo, DeviceState, OpenResult, PlanCommandSummary,
    RawPacketOutcome, SettingsState, ShutdownAck, StatusDto, UploadKind, UploadPlanSummary,
};
pub use error::IpcError;
pub use event::{Event, FrameValues, TelemetryStatus};
pub use frame::{decode, encode, encode_line, LineDecoder, DEFAULT_MAX_LINE};
pub use message::ServerMessage;
pub use request::{args, cmd, Request};
pub use response::{codes, hex, CommandError, PacketRecord, Response};

/// Bumped when a field is renamed or a message changes meaning. Additive fields do not
/// need a bump.
pub const PROTOCOL_VERSION: u32 = 1;
