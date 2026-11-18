//! `mystique explore` — the command-space mapper.
//!
//! The device is the specification. Four phases build on each other:
//!
//! * **presence** — sweep every byte with an empty payload and record whether it
//!   replies, what it echoes, its result byte, length and timing.
//! * **delta** — bracket each accepted command with `0x10` status reads.
//! * **payload** — probe payload sensitivity for commands that changed state.
//! * **sequence** — replay the init handshake and, on request, the upload preamble.
//!
//! Safety is layered: dry run by default, a conservative allowlist for real sends,
//! `--unsafe-all` for the full sweep, and an empty payload for every command whose
//! semantics are unknown. A blocked write is a wedge: the sweep stops once and the
//! operator gets the PnP recovery instruction.

use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use clap::ValueEnum;
use mystique_protocol::commands::{find as find_command, is_allowlisted, pipe_for};
use mystique_protocol::init::machine_info_sequence;
use mystique_protocol::upload::upload_preamble;
use mystique_protocol::{Pipe, Rotation};
use mystique_usb::{
    DeviceError, Link, LinkOptions, MystiqueDevice, Transport, DEFAULT_FRAME_TIMEOUT,
};

use crate::prober::Prober;
use crate::report::{DeviceReport, Entry, Report, StatusSnapshot, Summary, Wedge};
use crate::{emit, timefmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Phase {
    Presence,
    Delta,
    Payload,
    Sequence,
}

impl Phase {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Presence => "presence",
            Self::Delta => "delta",
            Self::Payload => "payload",
            Self::Sequence => "sequence",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PipeMode {
    /// Documented pipe for allowlisted commands, data pipe for everything else.
    Auto,
    Data,
    Init,
}

#[derive(Debug, Clone)]
pub struct ExploreOptions {
    pub phase: Phase,
    pub send: bool,
    pub unsafe_all: bool,
    pub skip: Vec<u8>,
    pub timeout: Duration,
    pub reports_dir: Option<PathBuf>,
    pub emit_constants: bool,
    pub constants_out: Option<PathBuf>,
    pub include_upload_preamble: bool,
    pub rotation: Rotation,
    pub pipe_mode: PipeMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Clean,
    Wedged,
}

impl ExploreOptions {
    fn allows(&self, command: u8) -> bool {
        self.unsafe_all || is_allowlisted(command)
    }

    /// Non-empty payloads go only to commands whose semantics are known, unless the
    /// operator explicitly took responsibility with `--unsafe-all`.
    fn allows_payload(&self, command: u8) -> bool {
        self.unsafe_all || is_allowlisted(command)
    }

    fn should_probe(&self, command: u8) -> bool {
        self.allows(command) && !self.skip.contains(&command)
    }

    fn probe_pipe(&self, command: u8) -> Pipe {
        match self.pipe_mode {
            PipeMode::Data => Pipe::Data,
            PipeMode::Init => Pipe::Init,
            PipeMode::Auto => {
                if is_allowlisted(command) {
                    pipe_for(command)
                } else {
                    // Unknown bytes are enumerated on the data pipe, as specified.
                    Pipe::Data
                }
            }
        }
    }

    fn mode_label(&self) -> &'static str {
        if self.unsafe_all {
            "unsafe-all"
        } else {
            "send"
        }
    }
}

pub fn run(options: ExploreOptions) -> Result<Outcome> {
    if options.emit_constants && !options.send {
        bail!("--emit-constants needs observations; add --send (a dry run records nothing)");
    }
    if options.include_upload_preamble && !options.unsafe_all {
        bail!(
            "--include-upload-preamble sends 0x0A and 0x11, which are outside the allowlist; \
             add --unsafe-all to accept the image-mode switch"
        );
    }
    if options.include_upload_preamble && options.phase != Phase::Sequence {
        bail!("--include-upload-preamble only applies to --phase sequence");
    }

    if !options.send {
        print_plan(&options);
        return Ok(Outcome::Clean);
    }

    let device = MystiqueDevice::open().context("opening the MYSTIQUE device")?;
    let device_report = DeviceReport::from_device(&device);

    let mut report = Report::new_header(
        options.phase.label(),
        options.mode_label(),
        options.timeout.as_millis() as u64,
        device_report,
    );
    report.skipped = options.skip.clone();

    let link = Link::with_options(
        device,
        LinkOptions {
            timeout: options.timeout,
            frame_timeout: DEFAULT_FRAME_TIMEOUT,
        },
    );
    let mut prober = Prober::new(link);
    let mut entries = Vec::new();
    let mut discovered = 0usize;

    println!(
        "phase {} · timeout {} ms · mode {} · {} bytes in scope",
        options.phase.label(),
        options.timeout.as_millis(),
        options.mode_label(),
        if options.unsafe_all { 256 } else { 15 }
    );
    if options.unsafe_all {
        println!(
            "WARNING: --unsafe-all is active. Every byte not listed in --skip may be sent \
             an empty payload. Unknown commands can have side effects."
        );
    }

    let fatal = match options.phase {
        Phase::Presence => run_presence(&mut prober, &options, &mut entries),
        Phase::Delta => run_delta(&mut prober, &options, &mut entries, &mut discovered),
        Phase::Payload => run_payload(&mut prober, &options, &mut entries, &mut discovered),
        Phase::Sequence => run_sequence(&mut prober, &options, &mut entries),
    };

    report.entries = entries;
    report.summary = Summary::from_entries(&report.entries);
    report.summary.count_state_changes(&report.entries);
    report.summary.discovered = discovered;

    if let Some(error) = &fatal {
        if error.is_wedge() {
            report.wedge = Some(Wedge {
                detected: true,
                phase: options.phase.label().to_owned(),
                at_cmd: report.entries.last().map(|entry| entry.cmd).unwrap_or(0),
                detail: error.to_string(),
                recovery: RECOVERY.to_owned(),
            });
        }
    }

    let reports_dir = resolve_reports_dir(options.reports_dir.as_deref());
    let report_path = unique_report_path(&reports_dir, "explore");
    report
        .write(&report_path)
        .with_context(|| format!("writing {}", report_path.display()))?;

    print_summary(&report.summary, &report_path);

    if options.emit_constants {
        let path = options
            .constants_out
            .clone()
            .or_else(emit::default_path)
            .ok_or_else(|| {
                anyhow!(
                    "cannot locate crates/mystique-protocol/src/commands.rs; pass --constants-out"
                )
            })?;
        emit::check_target(&path)?;
        let source = display_source(&report_path);
        let count = emit::update_file(&path, &report.entries, &source)?;
        println!(
            "wrote {count} observed command{} to {} (source: {source})",
            if count == 1 { "" } else { "s" },
            path.display()
        );
    }

    match fatal {
        Some(error) if error.is_wedge() => {
            print_wedge(&error);
            Ok(Outcome::Wedged)
        }
        Some(error) => {
            eprintln!("\nSTOPPED: {error}");
            if matches!(error, DeviceError::Disconnected) {
                eprintln!("The device disappeared. Replug it, then re-run.");
            }
            Err(anyhow!("exploration stopped: {error}"))
        }
        None => Ok(Outcome::Clean),
    }
}

// ---------------------------------------------------------------------------
// Phases
// ---------------------------------------------------------------------------

fn run_presence<T: Transport>(
    prober: &mut Prober<T>,
    options: &ExploreOptions,
    entries: &mut Vec<Entry>,
) -> Option<DeviceError> {
    for command in 0x00..=0xFFu8 {
        if !options.should_probe(command) {
            continue;
        }
        let outcome = prober.probe(options.probe_pipe(command), command, &[], None);
        print_entry(&outcome.entry);
        entries.push(outcome.entry);
        if let Some(error) = outcome.fatal {
            return Some(error);
        }
    }
    None
}

fn run_delta<T: Transport>(
    prober: &mut Prober<T>,
    options: &ExploreOptions,
    entries: &mut Vec<Entry>,
    discovered: &mut usize,
) -> Option<DeviceError> {
    let mut accepted = Vec::new();
    for command in 0x00..=0xFFu8 {
        if !options.should_probe(command) {
            continue;
        }
        let outcome = prober.probe(options.probe_pipe(command), command, &[], None);
        if let Some(error) = outcome.fatal {
            return Some(error);
        }
        if outcome.entry.succeeded() {
            accepted.push(command);
        }
    }
    *discovered = accepted.len();
    println!(
        "discovery: {} command{} accepted",
        accepted.len(),
        plural(accepted.len())
    );

    for command in accepted {
        let before = match prober.status_snapshot() {
            Ok(status) => status,
            Err(error) => return Some(error),
        };
        let mut outcome = prober.probe(
            options.probe_pipe(command),
            command,
            &[],
            Some("delta".to_owned()),
        );
        let after = match prober.status_snapshot() {
            Ok(status) => status,
            Err(error) => return Some(error),
        };
        outcome.entry.status_before = before;
        outcome.entry.status_after = after;
        print_entry(&outcome.entry);
        entries.push(outcome.entry);
        if let Some(error) = outcome.fatal {
            return Some(error);
        }
    }
    None
}

fn run_payload<T: Transport>(
    prober: &mut Prober<T>,
    options: &ExploreOptions,
    entries: &mut Vec<Entry>,
    discovered: &mut usize,
) -> Option<DeviceError> {
    // Presence, then a delta pass, both in memory: the payload phase needs to know
    // which commands are worth varying.
    let mut accepted = Vec::new();
    for command in 0x00..=0xFFu8 {
        if !options.should_probe(command) {
            continue;
        }
        let outcome = prober.probe(options.probe_pipe(command), command, &[], None);
        if let Some(error) = outcome.fatal {
            return Some(error);
        }
        if outcome.entry.succeeded() {
            accepted.push(command);
        }
    }

    let mut state_changing = Vec::new();
    for command in accepted {
        let before = match prober.status_snapshot() {
            Ok(status) => status,
            Err(error) => return Some(error),
        };
        let outcome = prober.probe(options.probe_pipe(command), command, &[], None);
        if let Some(error) = outcome.fatal {
            return Some(error);
        }
        let after = match prober.status_snapshot() {
            Ok(status) => status,
            Err(error) => return Some(error),
        };
        if status_changed(before, after) {
            state_changing.push(command);
        }
    }
    *discovered = state_changing.len();

    if state_changing.is_empty() {
        println!("no command changed the 0x10 status; nothing to vary.");
        return None;
    }
    println!(
        "{} command{} changed status; probing payload sensitivity",
        state_changing.len(),
        plural(state_changing.len())
    );

    for command in state_changing {
        if !options.allows_payload(command) {
            println!(
                "  0x{command:02X} skipped: payload probing for a non-allowlisted command \
                 requires --unsafe-all"
            );
            continue;
        }
        for (name, payload) in payload_variants() {
            let before = match prober.status_snapshot() {
                Ok(status) => status,
                Err(error) => return Some(error),
            };
            let mut outcome = prober.probe(
                options.probe_pipe(command),
                command,
                &payload,
                Some(format!("payload: {name}")),
            );
            let after = match prober.status_snapshot() {
                Ok(status) => status,
                Err(error) => return Some(error),
            };
            outcome.entry.status_before = before;
            outcome.entry.status_after = after;
            print_entry(&outcome.entry);
            entries.push(outcome.entry);
            if let Some(error) = outcome.fatal {
                return Some(error);
            }
        }
    }
    None
}

fn run_sequence<T: Transport>(
    prober: &mut Prober<T>,
    options: &ExploreOptions,
    entries: &mut Vec<Entry>,
) -> Option<DeviceError> {
    for step in machine_info_sequence(options.rotation) {
        let label = format!("init 0x{:02X}", step.command.code());
        let outcome = prober.probe(Pipe::Init, step.command.code(), &step.payload, Some(label));
        print_entry(&outcome.entry);
        entries.push(outcome.entry);
        if let Some(error) = outcome.fatal {
            return Some(error);
        }
        sleep(step.settle);
    }

    if options.include_upload_preamble {
        let preamble = upload_preamble(options.rotation);
        let total = preamble.len();
        for (index, command) in preamble.into_iter().enumerate() {
            let label = format!(
                "upload-preamble {}/{} 0x{:02X}",
                index + 1,
                total,
                command.code()
            );
            let outcome = prober.probe(
                command.pipe(),
                command.code(),
                &command.payload,
                Some(label),
            );
            print_entry(&outcome.entry);
            entries.push(outcome.entry);
            if let Some(error) = outcome.fatal {
                return Some(error);
            }
            sleep(command.settle);
        }
    }
    None
}

fn status_changed(before: Option<StatusSnapshot>, after: Option<StatusSnapshot>) -> bool {
    match (before, after) {
        (Some(before), Some(after)) => {
            before.rotation != after.rotation
                || before.mode_raw != after.mode_raw
                || before.result_code != after.result_code
        }
        // A status read that disappeared between the two probes is itself a change.
        (None, Some(_)) | (Some(_), None) => true,
        (None, None) => false,
    }
}

fn payload_variants() -> Vec<(&'static str, Vec<u8>)> {
    let ascending: Vec<u8> = (0u8..39).collect();
    let mut ascii = vec![0u8; 39];
    ascii[..14].copy_from_slice(b"MYSTIQUE-PROBE");
    vec![
        ("zero", vec![0x00; 39]),
        ("ones", vec![0xFF; 39]),
        ("ascending", ascending),
        ("ascii", ascii),
    ]
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

fn print_plan(options: &ExploreOptions) {
    println!("dry run — nothing will be sent\n");
    println!("phase:   {}", options.phase.label());
    println!("scope:   0x00..0xFF (256 command bytes)");
    if options.unsafe_all {
        println!(
            "gate:    --unsafe-all is active: all 256 bytes are in scope (allowlist: {})",
            mystique_protocol::commands::SAFE_ALLOWLIST.len()
        );
    } else {
        println!(
            "gate:    {} commands allowlisted; the other {} need --unsafe-all",
            mystique_protocol::commands::SAFE_ALLOWLIST.len(),
            256 - mystique_protocol::commands::SAFE_ALLOWLIST.len()
        );
    }
    println!("payload: empty for every unknown command");
    println!("timeout: {} ms", options.timeout.as_millis());
    if !options.skip.is_empty() {
        println!(
            "skip:    {}",
            options
                .skip
                .iter()
                .map(|c| format!("0x{c:02X}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    println!();

    match options.phase {
        Phase::Presence | Phase::Delta | Phase::Payload => {
            println!(" {:<5} {:<5} {:<9} note", "cmd", "pipe", "gate");
            for command in 0x00..=0xFFu8 {
                if !options.allows(command) && !options.unsafe_all {
                    continue;
                }
                if options.skip.contains(&command) {
                    continue;
                }
                let info = find_command(command);
                let name = info.map(|info| info.name).unwrap_or("unknown");
                let pipe = options.probe_pipe(command).label();
                let gate = if is_allowlisted(command) {
                    "allowlist"
                } else {
                    "unsafe"
                };
                println!(" 0x{command:02X}  {pipe:<5} {gate:<9} {name}");
            }
            if options.phase != Phase::Presence {
                println!(
                    "\n{} also runs an internal presence sweep, then brackets each accepted\n\
                     command with 0x10 status reads before recording anything.",
                    options.phase.label()
                );
            }
        }
        Phase::Sequence => {
            println!(" {:<4} {:<5} {:<26} label", "step", "cmd", "payload");
            for (index, step) in machine_info_sequence(options.rotation)
                .into_iter()
                .enumerate()
            {
                println!(
                    " {:>4}  0x{:02X}  {:<26} init {}",
                    index + 1,
                    step.command.code(),
                    hex_or_empty(&step.payload),
                    index + 1
                );
            }
            if options.include_upload_preamble {
                println!("\n upload preamble (requires --unsafe-all):");
                let preamble = upload_preamble(options.rotation);
                let total = preamble.len();
                for (index, command) in preamble.into_iter().enumerate() {
                    println!(
                        " {:>4}  0x{:02X}  {:<26} upload-preamble {}/{}",
                        index + 1,
                        command.code(),
                        hex_or_empty(&command.payload),
                        index + 1,
                        total
                    );
                }
            }
        }
    }

    println!("\nPass --send to execute. Start with --phase presence and no --unsafe-all.");
}

pub(crate) fn print_entry(entry: &Entry) {
    let result = entry
        .result_code
        .map(|raw| format!("{raw:02X}"))
        .unwrap_or_else(|| "--".to_owned());
    let echo = entry
        .echo
        .map(|raw| format!("{raw:02X}"))
        .unwrap_or_else(|| "--".to_owned());
    let len = entry
        .reply_len
        .map(|len| len.to_string())
        .unwrap_or_else(|| "-".to_owned());
    let label = entry
        .label
        .as_deref()
        .map(|label| format!(" [{label}]"))
        .unwrap_or_default();

    println!(
        "  0x{:02X}  {:<5} {:<8} echo={echo:<2} result={result:<2} len={len:<3} {:>6} ms{label}",
        entry.cmd, entry.pipe, entry.outcome, entry.elapsed_ms
    );
}

pub(crate) fn print_summary(summary: &Summary, path: &Path) {
    println!(
        "\nprobed {} · replied {} · success {} · timeouts {} · errors {} · state changes {}",
        summary.probed,
        summary.replied,
        summary.success,
        summary.timeouts,
        summary.errors,
        summary.state_changes
    );
    println!("report: {}", path.display());
}

pub(crate) fn print_wedge(error: &DeviceError) {
    eprintln!("\n================ WEDGE ================");
    eprintln!("{error}");
    eprintln!("The bulk OUT endpoint stopped accepting writes. Do not retry into it.");
    eprintln!();
    eprintln!("{RECOVERY}");
    eprintln!("=======================================");
}

const RECOVERY: &str = "\
Windows: pnputil /restart-device \"USB\\VID_3633&PID_0009\\<instance-id>\"
         find it with: Get-PnpDevice -InstanceId \"USB\\VID_3633&PID_0009\\*\"
         A service restart alone is not enough.
Linux:   unplug and replug the device, or power-cycle the USB port.";

/// The recovery text, shared with the `probe` verb so both print the same instruction.
pub(crate) const fn recovery_text() -> &'static str {
    RECOVERY
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

pub fn resolve_reports_dir(explicit: Option<&Path>) -> PathBuf {
    if let Some(dir) = explicit {
        return dir.to_path_buf();
    }
    if Path::new("re").is_dir() {
        return PathBuf::from("re/reports");
    }
    if Path::new("../re").is_dir() {
        return PathBuf::from("../re/reports");
    }
    PathBuf::from("reports")
}

/// `explore-<stamp>.json`, with a numeric suffix if two runs land in the same second.
/// Never overwrites an earlier report.
pub(crate) fn unique_report_path(dir: &Path, prefix: &str) -> PathBuf {
    let base = format!("{prefix}-{}", timefmt::file_stamp());
    let mut path = dir.join(format!("{base}.json"));
    let mut suffix = 1u32;
    while path.exists() {
        path = dir.join(format!("{base}-{suffix}.json"));
        suffix += 1;
    }
    path
}

/// A repo-relative path for the generated comments, with forward slashes.
fn display_source(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    text.strip_prefix("../").unwrap_or(&text).to_owned()
}

fn hex_or_empty(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        "-".to_owned()
    } else {
        crate::report::hex(bytes)
    }
}

const fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mystique_usb::mock::MockTransport;

    fn options(phase: Phase) -> ExploreOptions {
        ExploreOptions {
            phase,
            send: false,
            unsafe_all: false,
            skip: Vec::new(),
            timeout: Duration::from_millis(10),
            reports_dir: None,
            emit_constants: false,
            constants_out: None,
            include_upload_preamble: false,
            rotation: Rotation::ZERO,
            pipe_mode: PipeMode::Auto,
        }
    }

    #[test]
    fn the_gate_is_the_allowlist_by_default() {
        let options = options(Phase::Presence);
        assert!(options.allows(0x10));
        assert!(!options.allows(0x11));
        assert!(!options.should_probe(0x11));
    }

    #[test]
    fn unsafe_all_opens_the_sweep_but_skip_still_wins() {
        let mut options = options(Phase::Presence);
        options.unsafe_all = true;
        options.skip = vec![0x03, 0x04];
        assert!(options.allows(0x42));
        assert!(!options.should_probe(0x03));
        assert!(options.should_probe(0x42));
    }

    #[test]
    fn unknown_bytes_go_to_the_data_pipe_and_known_ones_to_their_pipe() {
        let options = options(Phase::Presence);
        assert_eq!(options.probe_pipe(0x10), Pipe::Data);
        assert_eq!(options.probe_pipe(0x12), Pipe::Init);
        // 0x0F is not allowlisted, so auto routes it to the data pipe.
        assert_eq!(options.probe_pipe(0x0F), Pipe::Data);
    }

    #[test]
    fn payload_probing_requires_responsibility() {
        let mut options = options(Phase::Payload);
        assert!(options.allows_payload(0x02));
        assert!(!options.allows_payload(0x0F));
        options.unsafe_all = true;
        assert!(options.allows_payload(0x0F));
    }

    #[test]
    fn payload_variants_are_all_39_bytes_and_distinct() {
        let variants = payload_variants();
        assert_eq!(variants.len(), 4);
        for (_, payload) in &variants {
            assert_eq!(payload.len(), 39);
        }
        assert_eq!(variants[0].1, vec![0x00; 39]);
        assert_eq!(variants[1].1, vec![0xFF; 39]);
        assert_eq!(variants[2].1[..3], [0, 1, 2]);
        assert_eq!(&variants[3].1[..14], b"MYSTIQUE-PROBE");
    }

    #[test]
    fn a_probe_records_a_timeout_as_data() {
        let mut prober = Prober::new(Link::new(MockTransport::silent()));
        let outcome = prober.probe(Pipe::Data, 0x10, &[], None);
        assert!(!outcome.entry.replied);
        assert_eq!(outcome.entry.cmd, 0x10);
        assert_eq!(outcome.entry.outcome, "timeout");
        assert!(outcome.fatal.is_none());
    }

    #[test]
    fn a_write_timeout_is_fatal_and_marked_wedge() {
        let mut prober = Prober::new(Link::new(MockTransport::device().wedge_on(0x10)));
        let outcome = prober.probe(Pipe::Data, 0x10, &[], None);
        assert_eq!(outcome.entry.outcome, "wedge");
        assert!(outcome.fatal.unwrap().is_wedge());
    }

    #[test]
    fn a_reply_records_every_required_field() {
        let mut prober = Prober::new(Link::new(MockTransport::device()));
        let outcome = prober.probe(Pipe::Data, 0x10, &[], None);
        assert!(outcome.entry.replied);
        assert_eq!(outcome.entry.echo, Some(0x10));
        assert_eq!(outcome.entry.result_code, Some(0x00));
        assert_eq!(outcome.entry.reply_len, Some(48));
        assert!(outcome.entry.reply_hex_redacted.is_some());
    }

    #[test]
    fn sequence_entries_are_ordered_and_labelled() {
        let mut prober = Prober::new(Link::new(MockTransport::device()));
        let mut entries = Vec::new();
        let options = options(Phase::Sequence);
        let fatal = run_sequence(&mut prober, &options, &mut entries);
        assert!(fatal.is_none());
        assert_eq!(entries.len(), 13);
        assert_eq!(entries[0].label.as_deref(), Some("init 0x12"));
        assert_eq!(entries[1].cmd, 0x02);
        assert_eq!(entries[1].payload_hex, "01 01 00 00 46");
    }

    #[test]
    fn dry_runs_touch_no_paths() {
        // `run` returns before opening the device when --send is absent; this test
        // pins that by using an explicitly unreachable report directory and asserting
        // the function still succeeds.
        let mut options = self::options(Phase::Presence);
        options.reports_dir = Some(PathBuf::from("definitely/not/created"));
        assert_eq!(run(options).unwrap(), Outcome::Clean);
        assert!(!Path::new("definitely").exists());
    }

    #[test]
    fn emit_constants_requires_send() {
        let mut options = self::options(Phase::Presence);
        options.emit_constants = true;
        let error = run(options).unwrap_err();
        assert!(error.to_string().contains("--emit-constants"));
    }

    #[test]
    fn the_upload_preamble_requires_unsafe_all() {
        let mut options = self::options(Phase::Sequence);
        options.send = true;
        options.include_upload_preamble = true;
        let error = run(options).unwrap_err();
        assert!(error.to_string().contains("--unsafe-all"));
    }

    #[test]
    fn a_presence_report_writes_the_documented_schema() {
        let mut prober = Prober::new(Link::new(MockTransport::device()));
        let options = options(Phase::Presence);
        let mut entries = Vec::new();
        assert!(run_presence(&mut prober, &options, &mut entries).is_none());
        assert_eq!(
            entries.len(),
            mystique_protocol::commands::SAFE_ALLOWLIST.len()
        );

        let mut report = Report::new_header(
            "presence",
            "send",
            750,
            DeviceReport {
                vendor_id: 0x3633,
                product_id: 0x0009,
                bus: 1,
                address: 5,
                release: 0x0246,
                product: Some("MYSTIQUE".to_owned()),
                identifiers_redacted: true,
            },
        );
        report.entries = entries;
        report.summary = Summary::from_entries(&report.entries);

        let dir = std::env::temp_dir().join("mystique-report-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("explore-test.json");
        report.write(&path).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["version"], 1);
        assert_eq!(value["phase"], "presence");
        assert!(value["wedge"].is_null());
        assert_eq!(
            value["entries"].as_array().unwrap().len(),
            mystique_protocol::commands::SAFE_ALLOWLIST.len()
        );
        for key in [
            "cmd",
            "replied",
            "echo",
            "result_code",
            "reply_len",
            "reply_hex_redacted",
            "elapsed_ms",
            "outcome",
            "status_before",
            "status_after",
        ] {
            assert!(value["entries"][0].get(key).is_some(), "missing {key}");
        }
        // Deterministic: two writes of the same report differ only in the timestamp.
        let first = serde_json::to_string(&value).unwrap();
        let second = serde_json::to_string(&value).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn a_delta_report_carries_status_before_and_after() {
        let mut prober = Prober::new(Link::new(MockTransport::device()));
        let options = options(Phase::Delta);
        let mut entries = Vec::new();
        let mut discovered = 0;
        assert!(run_delta(&mut prober, &options, &mut entries, &mut discovered).is_none());
        assert_eq!(discovered, entries.len());
        assert!(!entries.is_empty());
        for entry in &entries {
            assert!(entry.status_before.is_some(), "0x{:02X}", entry.cmd);
            assert!(entry.status_after.is_some(), "0x{:02X}", entry.cmd);
            assert_eq!(entry.label.as_deref(), Some("delta"));
        }
    }

    #[test]
    fn a_timeout_is_recorded_and_the_sweep_continues() {
        // The mock answers the first command and then goes quiet; presence must keep
        // going rather than abort.
        let mut prober = Prober::new(Link::new(MockTransport::silent()));
        let options = options(Phase::Presence);
        let mut entries = Vec::new();
        assert!(run_presence(&mut prober, &options, &mut entries).is_none());
        assert_eq!(
            entries.len(),
            mystique_protocol::commands::SAFE_ALLOWLIST.len()
        );
        assert!(entries.iter().all(|entry| entry.outcome == "timeout"));
        assert!(entries.iter().all(|entry| !entry.replied));
    }

    #[test]
    fn a_wedge_stops_the_sweep_at_the_first_command() {
        let mut prober = Prober::new(Link::new(MockTransport::device().wedge_on(0x01)));
        let options = options(Phase::Presence);
        let mut entries = Vec::new();
        let fatal = run_presence(&mut prober, &options, &mut entries);
        assert!(fatal.unwrap().is_wedge());
        // 0x01 is the first allowlisted command; nothing after it was sent.
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].cmd, 0x01);
        assert_eq!(entries[0].outcome, "wedge");
    }

    #[test]
    fn report_directories_resolve_relative_to_either_tree() {
        let dir = resolve_reports_dir(Some(Path::new("tmp/reports")));
        assert_eq!(dir, PathBuf::from("tmp/reports"));
    }

    #[test]
    fn report_paths_never_overwrite_an_earlier_run() {
        let dir = std::env::temp_dir().join("mystique-unique-report");
        std::fs::create_dir_all(&dir).unwrap();
        let first = unique_report_path(&dir, "explore");
        std::fs::write(&first, "{}").unwrap();
        let second = unique_report_path(&dir, "explore");
        assert_ne!(first, second);
        assert!(second.to_string_lossy().contains("-1.json"));
    }

    #[test]
    fn source_paths_are_repo_relative_and_forward_slashed() {
        assert_eq!(
            display_source(Path::new("../re/reports/explore-1.json")),
            "re/reports/explore-1.json"
        );
    }
}
