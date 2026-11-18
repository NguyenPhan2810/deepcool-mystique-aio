//! `mystique probe` — explicit command/payload/pipe triples.
//!
//! The presence sweep answers "does this byte reply". It cannot answer "is this
//! command payload-gated", "does it exist on this firmware", or "does it behave
//! differently on the other pipe". Those need a supplied payload on a chosen pipe,
//! which is what this verb is for.
//!
//! Nothing is defaulted. Every byte of every payload comes from the operator; there
//! is no built-in try list and no implicit payload. The only thing this module adds is
//! framing: header, `HIDC` trailer and checksum, which [`mystique_protocol::build_packet`]
//! always writes.
//!
//! A neutral `55 2E 00` ACK is an acknowledgement of receipt, not of understanding.
//! The `echo` field is the signal; `replied` alone is not.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use mystique_protocol::commands::is_allowlisted;
use mystique_protocol::packet::{build as build_packet, PACKET_LEN};
use mystique_protocol::{Pipe, ProtocolError};
use mystique_usb::{
    DeviceError, Link, LinkOptions, MystiqueDevice, Transport, DEFAULT_FRAME_TIMEOUT,
};

use crate::explore::{
    print_entry, print_summary, print_wedge, resolve_reports_dir, unique_report_path, Outcome,
};
use crate::prober::Prober;
use crate::report::{DeviceReport, Entry, Report, Scope, Summary, Wedge};

/// One operator-supplied probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeTry {
    pub command: u8,
    pub payload: Vec<u8>,
    pub pipe: Pipe,
}

impl ProbeTry {
    /// Parses `cmd_hex:payload_hex:init|data`.
    ///
    /// `cmd` is hexadecimal, with or without `0x`. An empty payload is written
    /// `0x10::data`; `-` is accepted as a synonym. The pipe is case-insensitive.
    pub fn parse(text: &str) -> Result<Self, String> {
        let parts: Vec<&str> = text.split(':').collect();
        if parts.len() != 3 {
            return Err(format!(
                "'{text}': expected <cmd_hex>:<payload_hex>:<init|data> \
                 (empty payload: 0x10::data)"
            ));
        }
        let command = parse_command(parts[0]).map_err(|error| format!("'{text}': {error}"))?;
        let payload = parse_payload(parts[1]).map_err(|error| format!("'{text}': {error}"))?;
        let pipe = parse_pipe(parts[2]).map_err(|error| format!("'{text}': {error}"))?;
        Ok(Self {
            command,
            payload,
            pipe,
        })
    }

    /// The exact 48-byte packet, checksum included. This is the only framing applied.
    pub fn packet(&self) -> Result<[u8; PACKET_LEN], ProtocolError> {
        build_packet(self.command, &self.payload)
    }
}

/// One line of a `--from-file` batch:
///
/// ```text
/// # comment
/// 0x0A ea07020202272100 init
/// 0x10 - data
/// 0x0F init
/// ```
pub fn load_file(path: &Path) -> Result<Vec<ProbeTry>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;

    let mut tries = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        let parsed = if line.contains(':') {
            ProbeTry::parse(line)
        } else {
            let fields: Vec<&str> = line.split_whitespace().collect();
            match fields.as_slice() {
                [command, payload, pipe] => ProbeTry::parse(&format!("{command}:{payload}:{pipe}")),
                [command, pipe] if parse_pipe(pipe).is_ok() => {
                    ProbeTry::parse(&format!("{command}::{pipe}"))
                }
                _ => Err("expected `cmd_hex payload_hex pipe` or `cmd_hex pipe`".to_owned()),
            }
        };

        tries.push(parsed.map_err(|error| anyhow!("{}:{}: {error}", path.display(), index + 1))?);
    }

    if tries.is_empty() {
        bail!("{} contains no probe lines", path.display());
    }
    Ok(tries)
}

#[derive(Debug, Clone)]
pub struct ProbeOptions {
    pub tries: Vec<ProbeTry>,
    pub send: bool,
    pub timeout: Duration,
    pub reports_dir: Option<PathBuf>,
}

pub fn run(options: ProbeOptions) -> Result<Outcome> {
    if options.tries.is_empty() {
        bail!("no probes: pass --try <cmd:payload:pipe> or --from-file <path>");
    }

    warn_outside_allowlist(&options.tries);

    if !options.send {
        print!("{}", render_dry_run(&options.tries)?);
        return Ok(Outcome::Clean);
    }

    let device = MystiqueDevice::open().context("opening the MYSTIQUE device")?;
    let mut report = Report::new_header(
        "probe",
        "send",
        options.timeout.as_millis() as u64,
        DeviceReport::from_device(&device),
    );
    report.scope = scope(&options.tries);

    let mut prober = Prober::new(Link::with_options(
        device,
        LinkOptions {
            timeout: options.timeout,
            frame_timeout: DEFAULT_FRAME_TIMEOUT,
        },
    ));

    let mut entries = Vec::new();
    let fatal = probe_all(&mut prober, &options.tries, &mut entries);

    report.entries = entries;
    report.summary = Summary::from_entries(&report.entries);
    if let Some(error) = &fatal {
        if error.is_wedge() {
            report.wedge = Some(Wedge {
                detected: true,
                phase: "probe".to_owned(),
                at_cmd: report.entries.last().map(|entry| entry.cmd).unwrap_or(0),
                detail: error.to_string(),
                recovery: crate::explore::recovery_text().to_owned(),
            });
        }
    }

    let reports_dir = resolve_reports_dir(options.reports_dir.as_deref());
    let report_path = unique_report_path(&reports_dir, "probe");
    report
        .write(&report_path)
        .with_context(|| format!("writing {}", report_path.display()))?;

    print_summary(&report.summary, &report_path);

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
            Err(anyhow!("probing stopped: {error}"))
        }
        None => Ok(Outcome::Clean),
    }
}

/// Sends every try in order, recording each. Stops on the first wedge or disconnect.
pub fn probe_all<T: Transport>(
    prober: &mut Prober<T>,
    tries: &[ProbeTry],
    entries: &mut Vec<Entry>,
) -> Option<DeviceError> {
    for attempt in tries {
        let outcome = prober.probe(attempt.pipe, attempt.command, &attempt.payload, None);
        print_entry(&outcome.entry);
        entries.push(outcome.entry);
        if let Some(error) = outcome.fatal {
            return Some(error);
        }
    }
    None
}

/// The exact plan, one line per packet, full hex and checksum included.
pub fn render_dry_run(tries: &[ProbeTry]) -> Result<String> {
    let mut out = String::new();
    out.push_str("dry run — nothing will be sent\n\n");
    out.push_str(" idx  cmd   pipe  payload  packet\n");
    for (index, attempt) in tries.iter().enumerate() {
        let packet = attempt.packet()?;
        out.push_str(&format!(
            " {:>3}  0x{:02X}  {:<4}  {:>5}B   {}\n",
            index + 1,
            attempt.command,
            attempt.pipe.label(),
            attempt.payload.len(),
            crate::report::hex(&packet)
        ));
    }
    out.push_str(&format!(
        "\n{} packet{} would be sent. Pass --send to transmit.\n",
        tries.len(),
        if tries.len() == 1 { "" } else { "s" }
    ));
    Ok(out)
}

fn warn_outside_allowlist(tries: &[ProbeTry]) {
    let mut outside: Vec<u8> = tries
        .iter()
        .map(|attempt| attempt.command)
        .filter(|command| !is_allowlisted(*command))
        .collect();
    outside.sort_unstable();
    outside.dedup();
    if outside.is_empty() {
        return;
    }
    let list = outside
        .iter()
        .map(|command| format!("0x{command:02X}"))
        .collect::<Vec<_>>()
        .join(", ");
    println!(
        "WARNING: {} probe{} outside the conservative allowlist: {list}",
        outside.len(),
        if outside.len() == 1 { "" } else { "s" }
    );
}

fn scope(tries: &[ProbeTry]) -> Scope {
    Scope {
        first: tries.iter().map(|t| t.command).min().unwrap_or(0),
        last: tries.iter().map(|t| t.command).max().unwrap_or(0),
        count: tries.len(),
    }
}

fn parse_command(text: &str) -> Result<u8, String> {
    let trimmed = text.trim();
    let hex = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    if hex.is_empty() {
        return Err("command is empty".to_owned());
    }
    u8::from_str_radix(hex, 16).map_err(|_| format!("command '{text}' is not a hex byte (00..FF)"))
}

fn parse_payload(text: &str) -> Result<Vec<u8>, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed == "-" {
        return Ok(Vec::new());
    }
    let compact: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    if !compact.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("payload '{text}' is not hexadecimal"));
    }
    if compact.len() % 2 != 0 {
        return Err(format!("payload '{text}' has an odd number of hex digits"));
    }
    let bytes = (0..compact.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&compact[index..index + 2], 16))
        .collect::<std::result::Result<Vec<u8>, _>>()
        .map_err(|_| format!("payload '{text}' is not hexadecimal"))?;
    if bytes.len() > mystique_protocol::MAX_PAYLOAD {
        return Err(format!(
            "payload is {} bytes; the maximum is {}",
            bytes.len(),
            mystique_protocol::MAX_PAYLOAD
        ));
    }
    Ok(bytes)
}

fn parse_pipe(text: &str) -> Result<Pipe, String> {
    match text.trim().to_ascii_lowercase().as_str() {
        "init" => Ok(Pipe::Init),
        "data" => Ok(Pipe::Data),
        other => Err(format!("pipe '{other}' is not 'init' or 'data'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mystique_protocol::checksum16;
    use mystique_usb::mock::MockTransport;

    fn try_of(text: &str) -> ProbeTry {
        ProbeTry::parse(text).unwrap()
    }

    #[test]
    fn parses_a_command_payload_and_pipe() {
        let parsed = try_of("0x0A:ea07020202272100:init");
        assert_eq!(parsed.command, 0x0A);
        assert_eq!(
            parsed.payload,
            vec![0xEA, 0x07, 0x02, 0x02, 0x02, 0x27, 0x21, 0x00]
        );
        assert_eq!(parsed.pipe, Pipe::Init);
    }

    #[test]
    fn an_empty_payload_is_spelled_two_ways() {
        assert_eq!(try_of("0x10::data").payload, Vec::<u8>::new());
        assert_eq!(try_of("0x10:-:data").payload, Vec::<u8>::new());
        assert_eq!(try_of("0x10::DATA").pipe, Pipe::Data);
    }

    #[test]
    fn command_is_hex_with_or_without_prefix() {
        assert_eq!(try_of("0a:00:init").command, 0x0A);
        assert_eq!(try_of("0X0A:00:init").command, 0x0A);
        assert_eq!(try_of("10:00:init").command, 0x10);
    }

    #[test]
    fn rejects_malformed_triples() {
        for text in [
            "",
            "0x10",
            "0x10:00",
            "0x10:00:data:extra",
            "0x100:00:data",
            "0xZZ:00:data",
            "0x10:0:data",
            "0x10:xyz:data",
            "0x10:00:usb",
        ] {
            assert!(ProbeTry::parse(text).is_err(), "{text} should be rejected");
        }
    }

    #[test]
    fn rejects_payloads_that_cannot_fit() {
        let too_long = format!("0x01:{}:data", "00".repeat(40));
        let error = ProbeTry::parse(&too_long).unwrap_err();
        assert!(error.contains("maximum"), "{error}");
    }

    #[test]
    fn builds_the_same_packet_as_the_captured_init_step() {
        let parsed = try_of("0x02:0101000046:init");
        let packet = parsed.packet().unwrap();
        let expected = "AA 2E 02 01 01 00 00 46 00 00 00 00 00 00 00 00 \
                        00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 \
                        00 00 00 00 00 00 00 00 00 00 48 49 44 43 3A 02";
        assert_eq!(crate::report::hex(&packet), expected);
        assert_eq!(
            u16::from_le_bytes([packet[46], packet[47]]),
            checksum16(&packet[..46])
        );
    }

    #[test]
    fn dry_run_prints_the_exact_full_packet() {
        let rendered = render_dry_run(&[try_of("0x02:0101000046:init")]).unwrap();
        assert!(rendered.contains("dry run"));
        assert!(rendered.contains(
            "AA 2E 02 01 01 00 00 46 00 00 00 00 00 00 00 00 \
             00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 \
             00 00 00 00 00 00 00 00 00 00 48 49 44 43 3A 02"
        ));
        // The checksum is included, not elided.
        let packet_line = rendered
            .lines()
            .find(|line| line.contains("AA 2E 02"))
            .expect("packet line");
        assert!(packet_line.ends_with("3A 02"), "{packet_line}");
        assert!(rendered.contains("1 packet would be sent"));
    }

    #[test]
    fn dry_run_keeps_the_operator_order() {
        let rendered = render_dry_run(&[
            try_of("0x0F::init"),
            try_of("0x10::data"),
            try_of("0x14::init"),
        ])
        .unwrap();
        let first = rendered.find("0x0F").unwrap();
        let second = rendered.find("0x10").unwrap();
        let third = rendered.find("0x14").unwrap();
        assert!(first < second && second < third);
    }

    #[test]
    fn reads_a_batch_file_in_order_with_comments_and_blank_lines() {
        let dir = std::env::temp_dir().join("mystique-probe-file");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("batch.txt");
        std::fs::write(
            &path,
            "# image commands on the init pipe\n\
             0x0F - init\n\
             \n\
             0x14 - init\n\
             0x10 data\n\
             0x0A ea07020202272100 init\n",
        )
        .unwrap();

        let tries = load_file(&path).unwrap();
        assert_eq!(tries.len(), 4);
        assert_eq!(tries[0].command, 0x0F);
        assert!(tries[0].payload.is_empty());
        assert_eq!(tries[1].command, 0x14);
        assert_eq!(tries[2].command, 0x10);
        assert_eq!(tries[2].pipe, Pipe::Data);
        assert_eq!(tries[3].command, 0x0A);
        assert_eq!(tries[3].payload.len(), 8);
    }

    #[test]
    fn a_bad_file_line_names_the_file_and_line() {
        let dir = std::env::temp_dir().join("mystique-probe-file");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.txt");
        std::fs::write(&path, "0x10 - data\n0xZZ - init\n").unwrap();

        let error = load_file(&path).unwrap_err().to_string();
        assert!(error.contains("bad.txt:2"), "{error}");
    }

    #[test]
    fn an_empty_batch_file_is_an_error() {
        let dir = std::env::temp_dir().join("mystique-probe-file");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("empty.txt");
        std::fs::write(&path, "# nothing here\n").unwrap();
        assert!(load_file(&path).is_err());
    }

    #[test]
    fn probes_every_try_in_order() {
        let mut prober = Prober::new(Link::new(MockTransport::device()));
        let tries = vec![try_of("0x10::data"), try_of("0x10::init")];
        let mut entries = Vec::new();
        assert!(probe_all(&mut prober, &tries, &mut entries).is_none());
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].pipe, "data");
        assert_eq!(entries[1].pipe, "init");
        assert_eq!(entries[0].payload_hex, "");
        assert!(entries.iter().all(|entry| entry.replied));
    }

    #[test]
    fn a_wedge_stops_the_batch_after_the_offending_command() {
        let mut prober = Prober::new(Link::new(MockTransport::device().wedge_on(0x0F)));
        let tries = vec![
            try_of("0x0F::init"),
            try_of("0x10::data"),
            try_of("0x12::init"),
        ];
        let mut entries = Vec::new();
        let fatal = probe_all(&mut prober, &tries, &mut entries);
        assert!(fatal.unwrap().is_wedge());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].cmd, 0x0F);
        assert_eq!(entries[0].outcome, "wedge");
    }

    #[test]
    fn no_tries_is_a_clear_error() {
        let error = run(ProbeOptions {
            tries: Vec::new(),
            send: false,
            timeout: Duration::from_millis(10),
            reports_dir: None,
        })
        .unwrap_err();
        assert!(error.to_string().contains("no probes"), "{error}");
    }

    #[test]
    fn report_names_use_the_probe_prefix() {
        let path = unique_report_path(Path::new("re/reports"), "probe");
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("probe-"), "{name}");
        assert!(name.ends_with(".json"), "{name}");
    }
}
