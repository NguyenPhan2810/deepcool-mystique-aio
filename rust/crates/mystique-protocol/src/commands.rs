//! The command table with per-command evidence grades.
//!
//! # How to read this
//!
//! The **pipe is part of the command's identity**. The 2026-11-18 sweep
//! (`re/findings/11-sweep-results.md`) showed that the init pipe echoes 18 bytes
//! and the data pipe only recognises `0x01`, `0x10` and — with a non-empty
//! payload — `0x0A`. Sending the right byte on the wrong pipe gets a neutral
//! `55 2E 00` ACK that looks like success.
//!
//! [`Command`] therefore carries the routing, not a free-standing byte. A caller
//! cannot ask for "command `0x0F` on the data pipe"; it asks for
//! [`Command::BeginUpload`], which only knows the init pipe.
//!
//! `presence` answers "does the device accept the byte at all?"; `semantics`
//! answers "do we know what it does?". A command that appears in the init
//! sequence has `presence = Verified` and `semantics = Unknown` — it exists, but
//! nobody has established its effect. Per `re/findings/09-protocol-mapping-plan.md`,
//! that is **not** a mapped command, and nothing here may invent a meaning for it.
//!
//! The block at the bottom of this file is generated from the reports under
//! `re/reports/`. Do not edit it by hand.

use serde::Serialize;

use crate::packet::{Pipe, MAX_PAYLOAD};
use crate::rotation::Rotation;

// ---------------------------------------------------------------------------
// Command bytes
// ---------------------------------------------------------------------------

/// `0x01` — telemetry frame. Verified: pipe `0x02/0x82`, 39-byte payload.
pub const CMD_TELEMETRY: u8 = 0x01;
/// `0x02` — display configuration. Verified: init step 2. The 360 capture sends
/// `01 01 ROT 00 46`; see [`display_config_payload`].
pub const CMD_DISPLAY_CONFIG: u8 = 0x02;
/// `0x0A` — the clock. The 360 capture always sends date and time
/// as `EA 07 <month> <day> <hour> <minute> <second>` about once a second; see
/// [`time_payload`] and [`crate::Clock`]. Earlier notes called this a mode switch with a
/// counter because the capture froze one date and hour. [`Command::ModeSwitch`] keeps
/// the historical typed name; the payload is the wall clock, not a mode or a counter.
/// Requires a non-empty payload: an empty `0x0A` is a silent no-op.
pub const CMD_MODE_SWITCH: u8 = 0x0A;
/// `0x0F` — begin image upload. Captured three times, immediately before the frames.
pub const CMD_BEGIN_UPLOAD: u8 = 0x0F;
/// `0x10` — status. Verified: pipe `0x02/0x82`, byte 4 rotation, byte 5 mode.
pub const CMD_STATUS: u8 = 0x10;
/// `0x11` — prior art calls this a serial request. Presence was verified on init, but
/// the **360 vendor never sends it**: it is absent from the 240-second capture. Kept
/// for the explorer only; no upload or init plan contains it.
pub const CMD_REQUEST_11: u8 = 0x11;
/// `0x12` — device info. Verified: pipe `0x01/0x81`, 32 hex identifier bytes.
pub const CMD_DEVICE_INFO: u8 = 0x12;
/// `0x14` — erase stored images. Captured once, between uploads; the 2026-11-18
/// hardware pass verified that it alone returns the panel from media to machine info.
pub const CMD_ERASE_IMAGES: u8 = 0x14;
/// `0xA1` — recognised on the init pipe only. Semantics unknown.
pub const CMD_UNKNOWN_A1: u8 = 0xA1;

/// Init step `0x08`. Verified as an init step; the upload commit uses payload `[01]`.
pub const CMD_INIT_STEP_08: u8 = 0x08;

/// `0x09` — observed twice in the 360 capture, payload `01` then `00`. **Semantics
/// unknown.** It is not in the init or upload sequence; do not invent a meaning.
pub const CMD_UNKNOWN_09: u8 = 0x09;

/// Init step `0x02` payload length.
pub const DISPLAY_CONFIG_LEN: usize = 5;

/// Builds the captured `0x02` payload: `01 01 ROT 00 46`.
///
/// Delegates to [`crate::display::DisplayConfig`], the single encoder for this
/// command, so the init block and the typed settings cannot drift apart. All five
/// bytes are now named: idle screen at 0, gyro at 1, rotation at 2, RGB sync at 3 and
/// brightness at 4 (`re/findings/13`, `17` and `18`).
///
/// The 2026-11-18 hardware pass (`re/findings/15-driver-verification.md`) disproved the
/// old reading of byte 0 as screen enable: writing `00` there does nothing, while
/// brightness `00` blanks the panel. Byte 0 is the idle screen toggle; the captured
/// `01` is kept so this helper stays byte-identical to the 360 init block.
pub fn display_config_payload(rotation: Rotation) -> [u8; DISPLAY_CONFIG_LEN] {
    crate::display::DisplayConfig::default()
        // The captured block has the gyro enabled; the settings builder's default is
        // off. The init and upload sequences must stay byte-identical to the capture,
        // so the captured bit is set here explicitly.
        .with_gyro(true)
        .with_rotation(rotation)
        .payload()
}

/// `0x03` payload in the captured init block: `01`. The settings capture correlated it
/// with the vendor's Monitor state, and `re/findings/17-mode-and-idle-screen.md` later
/// confirmed it as the display-mode selector (`01` Monitor, `02` Media, `03`
/// Recording). The driver does not send mode writes yet; the byte is included only as
/// one step of the captured handshake.
pub const INIT_STEP_03_PAYLOAD: [u8; 1] = [0x01];
/// `0x05` payload, unchanged from the 240 and seen throughout the 360 capture.
///
/// The two bytes are the recording graph options: index 0 CPU frequency, index 1 CPU
/// temperature (`re/findings/18-rgb-and-recording.md`). [`crate::display::RecordingConfig`]
/// is the typed encoder; this constant stays the captured default `[01 01]`.
pub const INIT_STEP_05_PAYLOAD: [u8; 2] = [0x01, 0x01];
/// `0x06` payload, unchanged from the 240 and seen throughout the 360 capture.
pub const INIT_STEP_06_PAYLOAD: [u8; 1] = [0x01];
/// `0x17` payload. The vendor sends `--` (two ASCII hyphens) in every block.
pub const INIT_STEP_17_PAYLOAD: [u8; 2] = [0x2D, 0x2D];

/// Full `0x0A` payload length: two header bytes plus month, day, hour, minute, second.
pub const TIME_PAYLOAD_LEN: usize = 7;

/// `0x0A` is the clock. Verified 2026-11-19 by capturing the vendor app at startup: the
/// payload is `EA 07 <month> <day> <hour> <minute> <second>`, sent about once a second,
/// and it tracks the wall clock. What earlier notes called a "counter" was the seconds
/// byte, and the `0B 12 0A` prefix was the capture's own date and hour frozen at that
/// moment (`re/findings/22-clock.md`).
pub const TIME_HEADER: [u8; 2] = [0xEA, 0x07];

/// Builds the seven `0x0A` bytes: `EA 07 <month> <day> <hour> <minute> <second>`.
///
/// There is no counter. [`crate::Clock`] keeps the five values together and is the type
/// the init sequence and the upload preamble take, so no call site can freeze one date.
pub const fn time_payload(
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
) -> [u8; TIME_PAYLOAD_LEN] {
    [
        TIME_HEADER[0],
        TIME_HEADER[1],
        month,
        day,
        hour,
        minute,
        second,
    ]
}

/// `0x09` payload observed first. **Semantics unknown.**
pub const UNKNOWN_09_PAYLOAD_ONE: [u8; 1] = [0x01];
/// `0x09` payload observed second. **Semantics unknown.**
pub const UNKNOWN_09_PAYLOAD_ZERO: [u8; 1] = [0x00];

/// Maximum length of an ASCII label carried by `0x15`, `0x16` or `0x17`.
pub const LABEL_MAX_LEN: usize = MAX_PAYLOAD;

/// Encodes a `0x15`/`0x16`/`0x17` text label.
///
/// The payload is ASCII, truncated to [`LABEL_MAX_LEN`] bytes. Characters outside
/// printable ASCII become `?`, so a multi-byte UTF-8 character can never be split and
/// the payload always fits one 48-byte packet.
pub fn label_payload(text: &str) -> Vec<u8> {
    text.chars()
        .map(|character| {
            if character.is_ascii_graphic() || character == ' ' {
                character as u8
            } else {
                b'?'
            }
        })
        .take(LABEL_MAX_LEN)
        .collect()
}

/// The twelve init commands other than `0x12`, in their observed order. Verified.
pub const INIT_SET: [u8; 12] = [
    0x02, 0x03, 0x04, 0x07, 0x08, 0x05, 0x0B, 0x06, 0x15, 0x16, 0x17, 0x0A,
];

/// Payloads from the 240-derived prior art that the 360 capture superseded.
///
/// **History only**: no code path reads these. They are kept so a future change can
/// see exactly what was replaced, and so nobody mistakes them for 360 facts.
pub mod legacy_240 {
    /// The old "Machine Info mode" payload. The 360 sends the local date and time on
    /// every `0x0A` instead (`re/findings/22-clock.md`).
    pub const MACHINE_INFO_MODE_PAYLOAD: [u8; 7] = [0xEA, 0x07, 0x02, 0x02, 0x02, 0x27, 0x21];
    /// The old `0x02` payload.
    pub const DISPLAY_CONFIG_PAYLOAD: [u8; 5] = [0x01, 0x00, 0x00, 0x01, 0x24];
    /// The old `0x04` payload, now understood as main `05` Temperature and aux `01`
    /// System Monitor; the 360 init capture sends all zeros instead.
    pub const INIT_STEP_04_PAYLOAD: [u8; 4] = [0x05, 0x00, 0x00, 0x01];
    /// The old `0x07` payload; the 360 sends an empty payload.
    pub const INIT_STEP_07_PAYLOAD: [u8; 2] = [0x00, 0x02];
    /// The old init `0x08` payload; the 360 sends an empty payload.
    pub const INIT_STEP_08_PAYLOAD: [u8; 2] = [0x00, 0x04];

    /// The old image-mode `0x0A` payload. The 360 sends the clock on every `0x0A`,
    /// before, during and after uploads.
    pub const UPLOAD_MODE_SWITCH_PAYLOAD: [u8; 8] =
        [0xEA, 0x07, 0x08, 0x09, 0x11, 0x3A, 0x09, 0x00];
    /// The old `0x01` display-update payload from the 240 preamble.
    pub const UPLOAD_DISPLAY_UPDATE_PAYLOAD: [u8; 9] =
        [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x54, 0x00, 0x1E];
    /// The old two-byte commit payload; the 360 commit is `01`.
    pub const UPLOAD_COMMIT_PAYLOAD: [u8; 2] = [0x01, 0x01];
}

/// Commands the explorer may send without `--unsafe-all`.
///
/// This is the documented conservative set: the four verified query commands plus
/// every command of the init sequence. It deliberately excludes `0x0F` and `0x11`,
/// which are inferred and only meaningful during an upload, `0x14` because erasing
/// deletes stored content, and the recognised-but-semantically-unknown bytes (`0x09`,
/// `0x0C`–`0x0E`, `0x13`, `0xA1`).
pub const SAFE_ALLOWLIST: [u8; 15] = [
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x0A, 0x0B, 0x10, 0x12, 0x15, 0x16, 0x17,
];

// ---------------------------------------------------------------------------
// Typed commands
// ---------------------------------------------------------------------------

/// A command that the driver knows how to route.
///
/// The variant, not the caller, decides the pipe. Bytes without a variant can
/// still be probed through [`crate::packet::build`] and the raw exchange API, but
/// they cannot enter an upload plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    Telemetry,
    DisplayConfig,
    SelectMode,
    InitStep04,
    InitStep05,
    InitStep06,
    InitStep07,
    InitStep08,
    Unknown09,
    /// `0x0A` inside the init handshake. The init pipe only acknowledges this byte;
    /// the panel's clock is driven by [`Command::Clock`] on the data pipe.
    ModeSwitch,
    /// `0x0A` as the vendor sends it about once a second: the wall clock, on the
    /// data pipe. The sweep found the init pipe ignores it even with a payload,
    /// while the data pipe honours it (`re/findings/22-clock.md`,
    /// `re/findings/11-sweep-results.md`).
    Clock,
    InitStep0B,
    Unknown0C,
    Unknown0D,
    Unknown0E,
    BeginUpload,
    Status,
    Request11,
    DeviceInfo,
    Unknown13,
    EraseImages,
    InitStep15,
    InitStep16,
    InitStep17,
    UnknownA1,
}

impl Command {
    pub const fn code(self) -> u8 {
        match self {
            Self::Telemetry => CMD_TELEMETRY,
            Self::DisplayConfig => CMD_DISPLAY_CONFIG,
            Self::SelectMode => 0x03,
            Self::InitStep04 => 0x04,
            Self::InitStep05 => 0x05,
            Self::InitStep06 => 0x06,
            Self::InitStep07 => 0x07,
            Self::InitStep08 => CMD_INIT_STEP_08,
            Self::Unknown09 => 0x09,
            Self::ModeSwitch => CMD_MODE_SWITCH,
            Self::Clock => CMD_MODE_SWITCH,
            Self::InitStep0B => 0x0B,
            Self::Unknown0C => 0x0C,
            Self::Unknown0D => 0x0D,
            Self::Unknown0E => 0x0E,
            Self::BeginUpload => CMD_BEGIN_UPLOAD,
            Self::Status => CMD_STATUS,
            Self::Request11 => CMD_REQUEST_11,
            Self::DeviceInfo => CMD_DEVICE_INFO,
            Self::Unknown13 => 0x13,
            Self::EraseImages => CMD_ERASE_IMAGES,
            Self::InitStep15 => 0x15,
            Self::InitStep16 => 0x16,
            Self::InitStep17 => 0x17,
            Self::UnknownA1 => CMD_UNKNOWN_A1,
        }
    }

    /// The pipe this command is sent on.
    ///
    /// `0x0A` is the awkward one. The working init sequence sends the clock on the init
    /// pipe, where the byte itself is silent; the data pipe echoes it only when the
    /// payload is non-empty, and no effect has been observed there. The driver keeps it
    /// on the init pipe, where the handshake is proven, and enforces the payload because
    /// an empty clock is a no-op on both.
    pub const fn pipe(self) -> Pipe {
        match self {
            Self::Telemetry | Self::Status | Self::Clock => Pipe::Data,
            _ => Pipe::Init,
        }
    }

    /// True when the firmware treats an empty payload as a silent no-op.
    pub const fn requires_payload(self) -> bool {
        matches!(self, Self::ModeSwitch | Self::Clock)
    }

    /// The known command with this byte, or `None` for an unmodelled byte.
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            CMD_TELEMETRY => Some(Self::Telemetry),
            CMD_DISPLAY_CONFIG => Some(Self::DisplayConfig),
            0x03 => Some(Self::SelectMode),
            0x04 => Some(Self::InitStep04),
            0x05 => Some(Self::InitStep05),
            0x06 => Some(Self::InitStep06),
            0x07 => Some(Self::InitStep07),
            CMD_INIT_STEP_08 => Some(Self::InitStep08),
            CMD_UNKNOWN_09 => Some(Self::Unknown09),
            CMD_MODE_SWITCH => Some(Self::ModeSwitch),
            0x0B => Some(Self::InitStep0B),
            0x0C => Some(Self::Unknown0C),
            0x0D => Some(Self::Unknown0D),
            0x0E => Some(Self::Unknown0E),
            CMD_BEGIN_UPLOAD => Some(Self::BeginUpload),
            CMD_STATUS => Some(Self::Status),
            CMD_REQUEST_11 => Some(Self::Request11),
            CMD_DEVICE_INFO => Some(Self::DeviceInfo),
            0x13 => Some(Self::Unknown13),
            CMD_ERASE_IMAGES => Some(Self::EraseImages),
            0x15 => Some(Self::InitStep15),
            0x16 => Some(Self::InitStep16),
            0x17 => Some(Self::InitStep17),
            CMD_UNKNOWN_A1 => Some(Self::UnknownA1),
            _ => None,
        }
    }
}

impl std::fmt::Display for Command {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "0x{:02X}", self.code())
    }
}

/// The documented pipe for a command byte.
///
/// Only meaningful for modelled bytes; anything else falls back to the init pipe,
/// which is the explorer's default for a byte it has no evidence about. The
/// explorer overrides this for unknown bytes and sends them on the data pipe.
pub const fn pipe_for(command: u8) -> Pipe {
    match Command::from_code(command) {
        Some(command) => command.pipe(),
        None => Pipe::Init,
    }
}

pub fn is_allowlisted(command: u8) -> bool {
    SAFE_ALLOWLIST.contains(&command)
}

// ---------------------------------------------------------------------------
// Evidence
// ---------------------------------------------------------------------------

/// How much is known about a fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Evidence {
    /// Observed on this 360, recorded under `re/fixtures/` or `re/reports/`.
    Verified,
    /// Recovered from vendor code or prior art. Presence may be verified, but the
    /// behaviour was not exercised here.
    Inferred,
    /// Named and present, semantics unresolved.
    Unknown,
}

impl Evidence {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Verified => "Verified",
            Self::Inferred => "Inferred",
            Self::Unknown => "Unknown",
        }
    }
}

// ---------------------------------------------------------------------------
// Hand-maintained table
// ---------------------------------------------------------------------------

/// One documented command.
#[derive(Debug, Clone, Copy)]
pub struct CommandInfo {
    pub code: u8,
    /// A neutral name. Commands whose effect is unknown are named `unknown-0xNN`
    /// rather than given a plausible-sounding role.
    pub name: &'static str,
    /// The pipe the driver sends this command on.
    pub pipe: Pipe,
    pub presence: Evidence,
    pub semantics: Evidence,
    pub note: &'static str,
}

impl CommandInfo {
    pub const fn is_fully_mapped(&self) -> bool {
        matches!(self.presence, Evidence::Verified) && matches!(self.semantics, Evidence::Verified)
    }
}

/// Ascending by command byte, so tables and reports diff cleanly.
pub const COMMANDS: &[CommandInfo] = &[
    CommandInfo {
        code: CMD_TELEMETRY,
        name: "telemetry",
        pipe: Pipe::Data,
        presence: Evidence::Verified,
        semantics: Evidence::Inferred,
        note: "39-byte payload; only the five display figures are understood",
    },
    CommandInfo {
        code: CMD_DISPLAY_CONFIG,
        name: "display-config",
        pipe: Pipe::Init,
        presence: Evidence::Verified,
        semantics: Evidence::Verified,
        note: "five absolute bytes: idle screen (0), gyro permission (1), rotation (2), \
               RGB light effect sync (3), brightness (4); every byte moved under a \
               labelled vendor action (re/findings/13, 17, 18); byte 0 is the idle \
               screen, not screen power; brightness 0 blanks the panel",
    },
    CommandInfo {
        code: 0x03,
        name: "display-mode",
        pipe: Pipe::Init,
        presence: Evidence::Verified,
        semantics: Evidence::Verified,
        note: "display mode selector: 01 Monitor, 02 Media, 03 Recording; four labelled \
               transitions in the 2026-11-18 capture (re/findings/17-mode-and-idle-screen.md); \
               stored media still dominates the panel while it exists, so 0x14 erase is the \
               way back from uploaded content",
    },
    CommandInfo {
        code: 0x04,
        name: "init-step-0x04",
        pipe: Pipe::Init,
        presence: Evidence::Verified,
        semantics: Evidence::Unknown,
        note: "four bytes: index 0 main, index 1 Style Type (00 Normal, 01 Ring, \
               02 Stopwatch), index 2 never observed non-zero and always written 00, \
               index 3 auxiliary style (00 Voltage Mode, 01 System Monitor, 02 Core \
               Data); the init capture is 00 00 00 00; style and aux render only with \
               main 00 CPU Frequency (re/findings/16-aux-area-layout.md)",
    },
    CommandInfo {
        code: 0x05,
        name: "recording-options",
        pipe: Pipe::Init,
        presence: Evidence::Verified,
        semantics: Evidence::Verified,
        note: "two bytes: index 0 CPU frequency graph, index 1 CPU temperature graph; \
               the captured init payload is 01 01, and both bits moved with the vendor \
               Recording toggles (re/findings/18-rgb-and-recording.md)",
    },
    CommandInfo {
        code: 0x06,
        name: "init-step-0x06",
        pipe: Pipe::Init,
        presence: Evidence::Verified,
        semantics: Evidence::Unknown,
        note: "init step, payload 01",
    },
    CommandInfo {
        code: 0x07,
        name: "init-step-0x07",
        pipe: Pipe::Init,
        presence: Evidence::Verified,
        semantics: Evidence::Unknown,
        note: "captured payload is empty on this 360; the settings capture maps index 0 \
               to image switch time (3/5/7 s) and index 1 to animation (Split/Scroll/Fade)",
    },
    CommandInfo {
        code: CMD_INIT_STEP_08,
        name: "init-step-0x08",
        pipe: Pipe::Init,
        presence: Evidence::Verified,
        semantics: Evidence::Unknown,
        note: "captured init payload is empty; the upload commit uses 01",
    },
    CommandInfo {
        code: CMD_UNKNOWN_09,
        name: "unknown-0x09",
        pipe: Pipe::Init,
        presence: Evidence::Verified,
        semantics: Evidence::Unknown,
        note: "captured twice with payload 01 then 00; not in either sequence; \
               semantics unknown",
    },
    CommandInfo {
        code: CMD_MODE_SWITCH,
        name: "clock",
        pipe: Pipe::Init,
        presence: Evidence::Verified,
        semantics: Evidence::Inferred,
        note: "the panel's clock: EA 07 <month> <day> <hour> <minute> <second>, sent \
               about once a second; what earlier notes read as a frozen prefix and a \
               big-endian counter was the capture's own date, hour and seconds, and the \
               payload tracks the wall clock (re/findings/22-clock.md); empty payload is \
               a silent no-op",
    },
    CommandInfo {
        code: 0x0B,
        name: "init-step-0x0B",
        pipe: Pipe::Init,
        presence: Evidence::Verified,
        semantics: Evidence::Unknown,
        note: "init step, empty payload",
    },
    CommandInfo {
        code: 0x0C,
        name: "unknown-0x0C",
        pipe: Pipe::Init,
        presence: Evidence::Verified,
        semantics: Evidence::Unknown,
        note: "recognised on init only; semantics unknown",
    },
    CommandInfo {
        code: 0x0D,
        name: "unknown-0x0D",
        pipe: Pipe::Init,
        presence: Evidence::Verified,
        semantics: Evidence::Unknown,
        note: "recognised on init only; semantics unknown",
    },
    CommandInfo {
        code: 0x0E,
        name: "unknown-0x0E",
        pipe: Pipe::Init,
        presence: Evidence::Verified,
        semantics: Evidence::Unknown,
        note: "recognised on init only; semantics unknown",
    },
    CommandInfo {
        code: CMD_BEGIN_UPLOAD,
        name: "begin-upload",
        pipe: Pipe::Init,
        presence: Evidence::Verified,
        semantics: Evidence::Inferred,
        note: "captured three times with an empty payload, immediately before the \
               frame headers",
    },
    CommandInfo {
        code: CMD_STATUS,
        name: "status",
        pipe: Pipe::Data,
        presence: Evidence::Verified,
        semantics: Evidence::Inferred,
        note: "byte 4 rotation, byte 5 mode; the mode byte is unreliable",
    },
    CommandInfo {
        code: CMD_REQUEST_11,
        name: "request-0x11",
        pipe: Pipe::Init,
        presence: Evidence::Verified,
        semantics: Evidence::Inferred,
        note: "240 prior art calls it a serial request; the 360 vendor never sends it; \
               the 38-byte body is all zeros, so no serial is parsed",
    },
    CommandInfo {
        code: CMD_DEVICE_INFO,
        name: "device-info",
        pipe: Pipe::Init,
        presence: Evidence::Verified,
        semantics: Evidence::Verified,
        note: "32 uppercase hex identifier bytes at 4..35, redacted by default",
    },
    CommandInfo {
        code: 0x13,
        name: "unknown-0x13",
        pipe: Pipe::Init,
        presence: Evidence::Verified,
        semantics: Evidence::Unknown,
        note: "recognised on init only; semantics unknown",
    },
    CommandInfo {
        code: CMD_ERASE_IMAGES,
        name: "erase-images",
        pipe: Pipe::Init,
        presence: Evidence::Verified,
        semantics: Evidence::Verified,
        note: "captured once between uploads; empty payload; erase alone returned the \
               panel from media to machine info on 2026-11-18 \
               (re/findings/15-driver-verification.md); required before a second upload \
               or the next one silently no-ops",
    },
    CommandInfo {
        code: 0x15,
        name: "init-step-0x15",
        pipe: Pipe::Init,
        presence: Evidence::Verified,
        semantics: Evidence::Unknown,
        note: "ASCII text label up to 39 bytes; the vendor sends the CPU name \
               (e.g. Intel Core i9-14900K); display effect not independently observed",
    },
    CommandInfo {
        code: 0x16,
        name: "init-step-0x16",
        pipe: Pipe::Init,
        presence: Evidence::Verified,
        semantics: Evidence::Unknown,
        note: "ASCII text label up to 39 bytes; the vendor sends the GPU name \
               (e.g. NVIDIA GeForce RTX 4070); display effect not independently observed",
    },
    CommandInfo {
        code: 0x17,
        name: "init-step-0x17",
        pipe: Pipe::Init,
        presence: Evidence::Verified,
        semantics: Evidence::Unknown,
        note: "ASCII text label; the vendor sends -- in every captured block",
    },
    CommandInfo {
        code: CMD_UNKNOWN_A1,
        name: "unknown-0xA1",
        pipe: Pipe::Init,
        presence: Evidence::Verified,
        semantics: Evidence::Unknown,
        note: "recognised on init only; newly discovered singleton; semantics unknown",
    },
];

pub fn find(command: u8) -> Option<&'static CommandInfo> {
    COMMANDS.iter().find(|info| info.code == command)
}

// ---------------------------------------------------------------------------
// Generated table
// ---------------------------------------------------------------------------

/// One command byte as observed by the sweep reports.
///
/// `pipes` lists the pipes that echoed the command byte back. An empty slice means
/// the command is silent everywhere it has been probed but still acts (the three
/// write-only init steps). `grade` is the **semantic** grade: recognition is proven
/// by `pipes`, behaviour is not.
#[derive(Debug, Clone, Copy)]
pub struct ObservedCommand {
    pub code: u8,
    /// Pipes where the command byte was echoed, with an empty payload unless
    /// `payload_gated` is set.
    pub pipes: &'static [Pipe],
    /// True when recognition needs a non-empty payload (`0x0A` on the data pipe).
    /// An empty payload is a silent no-op.
    pub payload_gated: bool,
    /// `Unknown` unless the behaviour is documented; never upgraded by an echo.
    pub grade: Evidence,
    /// Reports that produced this entry, relative to the repository root.
    pub sources: &'static [&'static str],
}

impl ObservedCommand {
    pub fn recognised_on(&self, pipe: Pipe) -> bool {
        self.pipes.contains(&pipe)
    }

    /// True for a command that never echoed but is part of a sequence that works.
    pub const fn is_silent_but_active(&self) -> bool {
        self.pipes.is_empty()
    }
}

// BEGIN GENERATED: observed-commands
//
// Generated from the sweep and probe reports below. Do not edit by hand.
// The count is 24: 21 echo-positive bytes across both pipes, plus the three
// silent-but-active init steps.
pub const OBSERVED_COMMANDS: &[ObservedCommand] = &[
    // 0x01 - echoed on data; 0x0A-style payload gating not applicable.
    ObservedCommand {
        code: 0x01,
        pipes: &[Pipe::Data],
        payload_gated: false,
        grade: Evidence::Inferred,
        sources: &[
            "re/reports/explore-20261117-175644.json",
            "re/reports/explore-20261117-175642.json",
        ],
    },
    // 0x02 - echoed on init; all five display-config bytes are now labelled.
    ObservedCommand {
        code: 0x02,
        pipes: &[Pipe::Init],
        payload_gated: false,
        grade: Evidence::Verified,
        sources: &[
            "re/reports/explore-20261117-175646.json",
            "re/reports/probe-20261117-175927.json",
        ],
    },
    // 0x03 - echoed on init; the display mode selector, verified by four transitions.
    ObservedCommand {
        code: 0x03,
        pipes: &[Pipe::Init],
        payload_gated: false,
        grade: Evidence::Verified,
        sources: &["re/reports/explore-20261117-175646.json"],
    },
    // 0x04 - echoed on init; semantics unknown.
    ObservedCommand {
        code: 0x04,
        pipes: &[Pipe::Init],
        payload_gated: false,
        grade: Evidence::Unknown,
        sources: &["re/reports/explore-20261117-175646.json"],
    },
    // 0x05 - echoed on init; recording options verified in the 2026-11-18 capture.
    ObservedCommand {
        code: 0x05,
        pipes: &[Pipe::Init],
        payload_gated: false,
        grade: Evidence::Verified,
        sources: &["re/reports/explore-20261117-175646.json"],
    },
    // 0x06 - echoed on init; semantics unknown.
    ObservedCommand {
        code: 0x06,
        pipes: &[Pipe::Init],
        payload_gated: false,
        grade: Evidence::Unknown,
        sources: &["re/reports/explore-20261117-175646.json"],
    },
    // 0x07 - echoed on init; semantics unknown.
    ObservedCommand {
        code: 0x07,
        pipes: &[Pipe::Init],
        payload_gated: false,
        grade: Evidence::Unknown,
        sources: &["re/reports/explore-20261117-175646.json"],
    },
    // 0x08 - echoed on init with an empty payload; 01 01 also echoes.
    ObservedCommand {
        code: 0x08,
        pipes: &[Pipe::Init],
        payload_gated: false,
        grade: Evidence::Unknown,
        sources: &[
            "re/reports/explore-20261117-175646.json",
            "re/reports/probe-20261117-175927.json",
        ],
    },
    // 0x09 - newly recognised on init; semantics unknown.
    ObservedCommand {
        code: 0x09,
        pipes: &[Pipe::Init],
        payload_gated: false,
        grade: Evidence::Unknown,
        sources: &["re/reports/explore-20261117-175646.json"],
    },
    // 0x0A - silent on init, payload-gated on data. An empty payload is a no-op.
    // The payload is the clock; the driver sends it on init, where the handshake is proven.
    ObservedCommand {
        code: 0x0A,
        pipes: &[Pipe::Data],
        payload_gated: true,
        grade: Evidence::Inferred,
        sources: &[
            "re/reports/probe-20261117-175927.json",
            "re/reports/probe-20261117-180556.json",
        ],
    },
    // 0x0B - echoed on init; semantics unknown.
    ObservedCommand {
        code: 0x0B,
        pipes: &[Pipe::Init],
        payload_gated: false,
        grade: Evidence::Unknown,
        sources: &["re/reports/explore-20261117-175646.json"],
    },
    // 0x0C - newly recognised on init; semantics unknown.
    ObservedCommand {
        code: 0x0C,
        pipes: &[Pipe::Init],
        payload_gated: false,
        grade: Evidence::Unknown,
        sources: &["re/reports/explore-20261117-175646.json"],
    },
    // 0x0D - newly recognised on init; semantics unknown.
    ObservedCommand {
        code: 0x0D,
        pipes: &[Pipe::Init],
        payload_gated: false,
        grade: Evidence::Unknown,
        sources: &["re/reports/explore-20261117-175646.json"],
    },
    // 0x0E - newly recognised on init; semantics unknown.
    ObservedCommand {
        code: 0x0E,
        pipes: &[Pipe::Init],
        payload_gated: false,
        grade: Evidence::Unknown,
        sources: &["re/reports/explore-20261117-175646.json"],
    },
    // 0x0F - presence verified on init; upload behaviour still inferred.
    ObservedCommand {
        code: 0x0F,
        pipes: &[Pipe::Init],
        payload_gated: false,
        grade: Evidence::Inferred,
        sources: &[
            "re/reports/explore-20261117-175646.json",
            "re/reports/probe-20261117-175927.json",
        ],
    },
    // 0x10 - echoed on data; rotation byte 4, mode byte 5.
    ObservedCommand {
        code: 0x10,
        pipes: &[Pipe::Data],
        payload_gated: false,
        grade: Evidence::Inferred,
        sources: &[
            "re/reports/explore-20261117-175644.json",
            "re/reports/probe-20261117-175927.json",
        ],
    },
    // 0x11 - presence verified on init; the body is all zeros on this unit.
    ObservedCommand {
        code: 0x11,
        pipes: &[Pipe::Init],
        payload_gated: false,
        grade: Evidence::Inferred,
        sources: &[
            "re/reports/explore-20261117-175646.json",
            "re/reports/probe-20261117-175927.json",
        ],
    },
    // 0x12 - echoed on init; the only command with understood identity data.
    ObservedCommand {
        code: 0x12,
        pipes: &[Pipe::Init],
        payload_gated: false,
        grade: Evidence::Verified,
        sources: &[
            "re/reports/explore-20261117-175646.json",
            "re/reports/explore-20261117-175644.json",
        ],
    },
    // 0x13 - newly recognised on init; semantics unknown.
    ObservedCommand {
        code: 0x13,
        pipes: &[Pipe::Init],
        payload_gated: false,
        grade: Evidence::Unknown,
        sources: &["re/reports/explore-20261117-175646.json"],
    },
    // 0x14 - presence verified on init; erase behaviour verified on hardware 2026-11-18.
    ObservedCommand {
        code: 0x14,
        pipes: &[Pipe::Init],
        payload_gated: false,
        grade: Evidence::Verified,
        sources: &[
            "re/reports/explore-20261117-175646.json",
            "re/reports/probe-20261117-175927.json",
        ],
    },
    // 0x15 - silent under an empty payload and two non-empty ones; write-only.
    ObservedCommand {
        code: 0x15,
        pipes: &[],
        payload_gated: false,
        grade: Evidence::Unknown,
        sources: &[
            "re/reports/explore-20261117-175646.json",
            "re/reports/probe-20261117-175927.json",
        ],
    },
    // 0x16 - silent under an empty payload and two non-empty ones; write-only.
    ObservedCommand {
        code: 0x16,
        pipes: &[],
        payload_gated: false,
        grade: Evidence::Unknown,
        sources: &[
            "re/reports/explore-20261117-175646.json",
            "re/reports/probe-20261117-175927.json",
        ],
    },
    // 0x17 - silent under an empty payload and two non-empty ones; write-only.
    ObservedCommand {
        code: 0x17,
        pipes: &[],
        payload_gated: false,
        grade: Evidence::Unknown,
        sources: &[
            "re/reports/explore-20261117-175646.json",
            "re/reports/probe-20261117-175927.json",
        ],
    },
    // 0xA1 - newly discovered singleton on init; semantics unknown.
    ObservedCommand {
        code: 0xA1,
        pipes: &[Pipe::Init],
        payload_gated: false,
        grade: Evidence::Unknown,
        sources: &[
            "re/reports/explore-20261117-175646.json",
            "re/reports/probe-20261117-175927.json",
        ],
    },
];
// END GENERATED: observed-commands

pub fn observed(command: u8) -> Option<&'static ObservedCommand> {
    OBSERVED_COMMANDS.iter().find(|entry| entry.code == command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted_and_unique() {
        let mut previous = None;
        for info in COMMANDS {
            if let Some(previous) = previous {
                assert!(info.code > previous, "0x{:02X} is out of order", info.code);
            }
            previous = Some(info.code);
        }
    }

    #[test]
    fn observed_table_is_sorted_and_unique() {
        let mut previous = None;
        for info in OBSERVED_COMMANDS {
            if let Some(previous) = previous {
                assert!(info.code > previous, "0x{:02X} is out of order", info.code);
            }
            previous = Some(info.code);
        }
    }

    #[test]
    fn allowlist_is_the_documented_set() {
        let mut allow = SAFE_ALLOWLIST.to_vec();
        allow.sort_unstable();
        allow.dedup();
        assert_eq!(
            allow,
            vec![
                0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x0A, 0x0B, 0x10, 0x12, 0x15, 0x16,
                0x17
            ]
        );
        assert!(is_allowlisted(0x10));
        assert!(!is_allowlisted(0x0F));
        assert!(!is_allowlisted(0x11));
        assert!(!is_allowlisted(0x14));
        assert!(!is_allowlisted(0x00));
    }

    #[test]
    fn every_init_command_is_allowlisted() {
        for command in INIT_SET {
            assert!(is_allowlisted(command), "0x{command:02X}");
        }
    }

    #[test]
    fn every_tabled_command_has_a_typed_variant() {
        for info in COMMANDS {
            let command = Command::from_code(info.code)
                .unwrap_or_else(|| panic!("0x{:02X} has no Command variant", info.code));
            assert_eq!(command.pipe(), info.pipe, "0x{:02X} pipe", info.code);
            assert_eq!(command.code(), info.code);
        }
    }

    #[test]
    fn typed_commands_route_by_observation() {
        assert_eq!(Command::Status.pipe(), Pipe::Data);
        assert_eq!(Command::Telemetry.pipe(), Pipe::Data);
        assert_eq!(Command::DeviceInfo.pipe(), Pipe::Init);
        assert_eq!(Command::DisplayConfig.pipe(), Pipe::Init);
        assert_eq!(Command::BeginUpload.pipe(), Pipe::Init);
        // The image commands live on the init pipe. This is the routing the
        // upload plan depends on.
        assert_eq!(Command::EraseImages.pipe(), Pipe::Init);
        assert_eq!(Command::Request11.pipe(), Pipe::Init);
        assert_eq!(pipe_for(0x42), Pipe::Init);
    }

    #[test]
    fn the_observed_table_is_the_measured_union() {
        let echoed: Vec<u8> = OBSERVED_COMMANDS
            .iter()
            .filter(|entry| !entry.pipes.is_empty())
            .map(|entry| entry.code)
            .collect();
        assert_eq!(
            echoed,
            vec![
                0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E,
                0x0F, 0x10, 0x11, 0x12, 0x13, 0x14, 0xA1,
            ]
        );
        assert_eq!(echoed.len(), 21);
        assert_eq!(OBSERVED_COMMANDS.len(), 24);
    }

    #[test]
    fn only_the_three_write_only_steps_are_silent() {
        let silent: Vec<u8> = OBSERVED_COMMANDS
            .iter()
            .filter(|entry| entry.is_silent_but_active())
            .map(|entry| entry.code)
            .collect();
        assert_eq!(silent, vec![0x15, 0x16, 0x17]);
    }

    #[test]
    fn only_the_clock_is_payload_gated() {
        let gated: Vec<u8> = OBSERVED_COMMANDS
            .iter()
            .filter(|entry| entry.payload_gated)
            .map(|entry| entry.code)
            .collect();
        assert_eq!(gated, vec![0x0A]);
        let clock = observed(0x0A).unwrap();
        assert!(clock.recognised_on(Pipe::Data));
        assert!(!clock.recognised_on(Pipe::Init));
        assert!(Command::ModeSwitch.requires_payload());
    }

    #[test]
    fn the_periodic_clock_takes_the_data_pipe() {
        // The sweep found the panel honours 0x0A on the data pipe only; the init pipe
        // answers with a neutral ACK and the panel's own clock keeps running
        // (re/findings/22-clock.md). Sending the wall clock on the init pipe therefore
        // looked successful and changed nothing.
        assert_eq!(Command::Clock.code(), CMD_MODE_SWITCH);
        assert_eq!(Command::Clock.pipe(), Pipe::Data);
        assert!(Command::Clock.requires_payload());
        assert_eq!(Command::ModeSwitch.pipe(), Pipe::Init);
        assert_eq!(Command::Clock.code(), Command::ModeSwitch.code());
    }

    #[test]
    fn grades_never_promote_a_command_whose_behaviour_was_not_exercised() {
        for code in [0x0F, 0x11] {
            assert_eq!(
                observed(code).unwrap().grade,
                Evidence::Inferred,
                "0x{code:02X}"
            );
        }
        // Erase is no longer merely inferred: the 2026-11-18 hardware pass sent `0x14`
        // alone and the panel left stored media for machine info.
        assert_eq!(observed(0x14).unwrap().grade, Evidence::Verified);
        // The 2026-11-18 settings captures labelled the remaining display bytes.
        assert_eq!(observed(0x02).unwrap().grade, Evidence::Verified);
        assert_eq!(observed(0x03).unwrap().grade, Evidence::Verified);
        assert_eq!(observed(0x05).unwrap().grade, Evidence::Verified);
        for code in [0x04, 0x06, 0x07, 0x09, 0x0B, 0x0C, 0x0D, 0x0E, 0x13, 0xA1] {
            assert_eq!(
                observed(code).unwrap().grade,
                Evidence::Unknown,
                "0x{code:02X}"
            );
        }
        assert_eq!(observed(0x0F).unwrap().pipes, &[Pipe::Init]);
        assert_eq!(observed(0xA1).unwrap().pipes, &[Pipe::Init]);
    }

    #[test]
    fn observed_and_hand_tables_agree() {
        for entry in OBSERVED_COMMANDS {
            let info = find(entry.code)
                .unwrap_or_else(|| panic!("0x{:02X} is observed but not tabled", entry.code));
            assert_eq!(info.semantics, entry.grade, "0x{:02X} grade", entry.code);
        }
        assert!(!find(0xA1).unwrap().is_fully_mapped());
    }

    #[test]
    fn nothing_unobserved_is_called_mapped() {
        for info in COMMANDS {
            if !matches!(info.presence, Evidence::Verified) {
                assert!(!info.is_fully_mapped(), "0x{:02X}", info.code);
            }
        }
        // Device-info, erase and the three labelled settings commands have both presence
        // and semantics verified.
        let mapped: Vec<u8> = COMMANDS
            .iter()
            .filter(|info| info.is_fully_mapped())
            .map(|info| info.code)
            .collect();
        assert_eq!(mapped, vec![0x02, 0x03, 0x05, 0x12, 0x14]);
    }

    #[test]
    fn the_0x03_selector_is_the_verified_display_mode() {
        let info = find(0x03).unwrap();
        assert_eq!(info.semantics, Evidence::Verified);
        assert!(info.note.contains("Monitor"));
        assert!(info.note.contains("Recording"));
        assert!(info.note.contains("0x14"));
    }

    #[test]
    fn every_source_is_a_report_under_re_reports() {
        for entry in OBSERVED_COMMANDS {
            assert!(!entry.sources.is_empty(), "0x{:02X}", entry.code);
            for source in entry.sources {
                assert!(source.starts_with("re/reports/"), "{source}");
                assert!(source.ends_with(".json"), "{source}");
            }
        }
    }

    #[test]
    fn display_config_payload_matches_the_capture() {
        assert_eq!(
            display_config_payload(Rotation::ZERO),
            [0x01, 0x01, 0x00, 0x00, 0x46]
        );
        assert_eq!(
            display_config_payload(Rotation::new(2).unwrap()),
            [0x01, 0x01, 0x02, 0x00, 0x46]
        );
    }

    #[test]
    fn the_clock_payload_header_and_field_order_are_the_capture_shape() {
        // re/captures/usbpcap-time-sync.pcap, 2026-11-19 10:11:46. The point of the
        // test is the layout, not that one date: the header is fixed and the five
        // values follow in calendar order.
        let payload = time_payload(11, 19, 10, 11, 46);
        assert_eq!(payload.len(), TIME_PAYLOAD_LEN);
        assert_eq!(&payload[..2], &TIME_HEADER);
        assert_eq!(payload[2..], [11, 19, 10, 11, 46]);
    }

    #[test]
    fn labels_are_ascii_and_truncate_at_thirty_nine_bytes() {
        assert_eq!(
            label_payload("Intel Core i9-14900K"),
            b"Intel Core i9-14900K"
        );
        assert_eq!(label_payload("--"), b"--");
        let long = "A".repeat(LABEL_MAX_LEN + 20);
        assert_eq!(label_payload(&long).len(), LABEL_MAX_LEN);
        assert_eq!(label_payload(&long), vec![b'A'; LABEL_MAX_LEN]);

        // Non-ASCII becomes '?', one byte per character, so no UTF-8 sequence is split.
        let mixed = label_payload("CPU 温度");
        assert!(mixed.iter().all(u8::is_ascii));
        assert_eq!(mixed, b"CPU ??".to_vec());

        assert!(label_payload("a\tb\nc").iter().all(u8::is_ascii));
        assert_eq!(label_payload("a\tb\nc"), b"a?b?c".to_vec());
    }

    #[test]
    fn unknown_09_keeps_both_observed_payloads_and_no_meaning() {
        assert_eq!(UNKNOWN_09_PAYLOAD_ONE, [0x01]);
        assert_eq!(UNKNOWN_09_PAYLOAD_ZERO, [0x00]);
        let info = find(CMD_UNKNOWN_09).unwrap();
        assert_eq!(info.semantics, Evidence::Unknown);
        assert!(info.note.contains("unknown"));
    }

    #[test]
    fn legacy_240_payloads_are_documented_but_never_reachable_from_the_sequence() {
        // The module exists for history. This test pins the values so a future edit
        // cannot silently turn them back into the live payloads.
        assert_eq!(
            legacy_240::MACHINE_INFO_MODE_PAYLOAD,
            [0xEA, 0x07, 0x02, 0x02, 0x02, 0x27, 0x21]
        );
        assert_eq!(
            legacy_240::DISPLAY_CONFIG_PAYLOAD,
            [0x01, 0x00, 0x00, 0x01, 0x24]
        );
        assert_eq!(legacy_240::INIT_STEP_08_PAYLOAD, [0x00, 0x04]);
        assert_eq!(
            legacy_240::UPLOAD_MODE_SWITCH_PAYLOAD,
            [0xEA, 0x07, 0x08, 0x09, 0x11, 0x3A, 0x09, 0x00]
        );
        assert_eq!(
            legacy_240::UPLOAD_DISPLAY_UPDATE_PAYLOAD,
            [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x54, 0x00, 0x1E]
        );
        assert_eq!(legacy_240::UPLOAD_COMMIT_PAYLOAD, [0x01, 0x01]);
        assert_ne!(
            crate::Clock::zero().payload(),
            legacy_240::MACHINE_INFO_MODE_PAYLOAD
        );
        assert_ne!(
            display_config_payload(Rotation::ZERO),
            legacy_240::DISPLAY_CONFIG_PAYLOAD
        );
    }
}
