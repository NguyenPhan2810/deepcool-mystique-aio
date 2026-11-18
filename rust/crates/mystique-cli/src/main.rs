//! The `mystique` CLI.
//!
//! Verbs mirror the existing Windows tool (`pipes`, `status`, `info`, `init`, `stream`,
//! `live`) so muscle memory transfers, plus `image`, `gif`, and — the important one —
//! `explore`, which maps the device's command language from observation only.

#![forbid(unsafe_code)]

mod emit;
mod explore;
mod probe;
mod prober;
mod report;
mod timefmt;

use std::path::PathBuf;
use std::process::ExitCode;
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use mystique_media as image;
use mystique_protocol::{
    build_upload_plan, commands::CMD_DEVICE_INFO, rotate_with_gyro_off, set_brightness,
    set_display_areas, set_gyro, set_idle_screen, set_media_timing, CommandPacket, ContentId,
    DeviceInfo, DisplayAreas, MachineLabels, MediaTiming, Operation, Pipe, ResultCode, Rotation,
    TelemetryFrame, UploadRequest,
};
use mystique_usb::{DeviceError, Link, MystiqueDevice};

const ABOUT: &str = "Drive the DeepCool MYSTIQUE AIO LCD (3633:0009) and map its protocol.";

const EXPLORE_LONG_ABOUT: &str = "\
Map the device's command language from observation. This is the tool that completes the
protocol without the vendor app.

Four phases, each building on the previous one:

  presence  sweep command bytes with an empty payload. Records whether a reply arrived,
            the echoed byte, the result code, reply length and elapsed time.
  delta     do the same, then bracket every accepted command with 0x10 status reads and
            record the before/after pair. This is how state changes are found.
  payload   for commands that changed status, probe four payloads (zero, 0xFF,
            ascending, ASCII) and record reply and status changes.
  sequence  replay the 13-step init handshake and, with --include-upload-preamble,
            the image-upload preamble. This produces regression fixtures.

Why empty payloads: the firmware accepts arbitrary payload bytes without validating
them (proven on this unit), so an empty payload is the lowest-risk probe. Unknown
commands may still have side effects, which is why nothing is sent unless --send is
given and only the conservative allowlist is sent unless --unsafe-all is also given.

A timeout is data, not a failure: it is recorded with its duration and the sweep
continues. A blocked write is a wedge: the sweep stops after the first one and prints
the PnP recovery instruction. It is never retried.";

const PROBE_LONG_ABOUT: &str = "\
Send explicit command/payload/pipe triples and record the replies.

Every payload byte comes from you. There is no built-in try list and no default
payload; the only framing applied is the 48-byte header, HIDC trailer and checksum.

A neutral `55 2E 00` ACK means the packet was received, not understood. The echo byte
is the signal, and a command can act without echoing (0x0A, 0x15, 0x16 and 0x17 are in
the working init sequence yet reply with echo 0x00).

Examples:
  mystique probe --send --try 0x0A:ea070b120a2c3a:init --try 0x15:496e74656c20436f72652069392d31343930304b:init
  mystique probe --from-file probes.txt --send
  mystique probe --try 0x0F::init --try 0x0F::data

Without --send nothing is transmitted: the exact 48-byte packet for every try is
printed, checksum included. A blocked write stops the batch after that command and
prints the recovery instruction.";

#[derive(Parser)]
#[command(name = "mystique", version, about = ABOUT, long_about = ABOUT)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the USB endpoints and descriptor summary.
    Pipes,

    /// Read rotation and mode (0x10). Advisory: the mode byte is unreliable.
    Status,

    /// Read the device identifier (0x12). Redacted unless --show-device-id.
    Info {
        /// Print the per-device identifier. Never paste this into an issue or report.
        #[arg(long)]
        show_device_id: bool,
    },

    /// Run the 13-command Machine Info handshake.
    Init {
        /// 0 = 0°, 1 = 90°, 2 = 180°, 3 = 270°.
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=3), default_value_t = 0)]
        rotation: u8,
        /// CPU name sent by 0x15 (ASCII, truncated to 39 bytes).
        #[arg(long)]
        cpu: Option<String>,
        /// GPU name sent by 0x16 (ASCII, truncated to 39 bytes).
        #[arg(long)]
        gpu: Option<String>,
    },

    /// Send synthetic telemetry at 1 Hz. 0 seconds means until Ctrl+C.
    Stream {
        #[arg(long, default_value_t = 30)]
        seconds: u64,
    },

    /// Set panel brightness (0x02 index 4, 0-100).
    ///
    /// Sends the whole five-byte display config, so screen, gyro and rotation are
    /// written at their defaults too. The vendor service must be stopped to reach the
    /// device: the panel is single-owner.
    Brightness {
        #[arg(value_parser = clap::value_parser!(u8).range(0..=100))]
        value: u8,
        /// Actually write. Without this the packet is printed and nothing is sent.
        #[arg(long)]
        send: bool,
    },

    /// Rotate the panel (0x02 index 2, 0/90/180/270 degrees).
    ///
    /// The gyroscopes are switched off in the same payload, because the device ignores
    /// rotation while they are enabled. The vendor service must be stopped to reach the
    /// device: the panel is single-owner.
    Rotate {
        /// 0 = 0 degrees, 1 = 90, 2 = 180, 3 = 270.
        #[arg(value_parser = clap::value_parser!(u8).range(0..=3))]
        rotation: u8,
        /// Actually write. Without this the packet is printed and nothing is sent.
        #[arg(long)]
        send: bool,
    },

    /// Allow or block the panel gyroscopes (0x02 index 1).
    ///
    /// The vendor service must be stopped to reach the device: the panel is
    /// single-owner.
    Gyro {
        /// on = gyroscopes allowed, off = gyroscopes disabled.
        #[arg(value_enum)]
        state: OnOff,
        /// Actually write. Without this the packet is printed and nothing is sent.
        #[arg(long)]
        send: bool,
    },

    /// Write the unknown `0x02` index-0 byte. **This is not screen control.**
    ///
    /// The 2026-11-18 hardware pass proved the byte does nothing visible, while
    /// brightness 0 blanks the panel. The verb exists so the byte can be exercised
    /// deliberately; use `brightness 0` to turn the screen off.
    ///
    /// The vendor service must be stopped to reach the device: the panel is
    /// single-owner.
    Flag0 {
        /// on = 0x02 byte 0 is 01, off = 00. Effect unknown.
        #[arg(value_enum)]
        state: OnOff,
        /// Actually write. Without this the packet is printed and nothing is sent.
        #[arg(long)]
        send: bool,
    },

    /// Set the main, Style Type and auxiliary display areas (0x04).
    ///
    /// The payload is four bytes: main at index 0, Style Type at index 1, index 2
    /// reserved and always zero, auxiliary style at index 3. Main and aux accept
    /// 0..=15 because the findings do not define a closed list; Style Type is the
    /// closed 0..=2 list (0 Normal, 1 Ring, 2 Stopwatch). Style and aux only have a
    /// visible effect when main is 0 (CPU Frequency). The vendor service must be
    /// stopped to reach the device: the panel is single-owner.
    Areas {
        /// Main Display Area selector (0x04 index 0).
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=15))]
        main: u8,
        /// Style Type selector (0x04 index 1): 0 Normal, 1 Ring, 2 Stopwatch.
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=2), default_value_t = 0)]
        style: u8,
        /// Auxiliary Display Area style selector (0x04 index 3).
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=15))]
        aux: u8,
        /// Actually write. Without this the packet is printed and nothing is sent.
        #[arg(long)]
        send: bool,
    },

    /// Set image switch time and animation (0x07).
    ///
    /// The vendor service must be stopped to reach the device: the panel is
    /// single-owner.
    MediaTiming {
        /// 0 = 3 s, 1 = 5 s, 2 = 7 s.
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=2))]
        time: u8,
        /// 0 = Split Display, 1 = Scroll, 2 = Fade In/Out.
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=2))]
        animation: u8,
        /// Actually write. Without this the packet is printed and nothing is sent.
        #[arg(long)]
        send: bool,
    },

    /// Run init, then stream real sensor readings at 1 Hz. 0 count means until Ctrl+C.
    Live {
        #[arg(long, default_value_t = 0)]
        count: u64,
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=3), default_value_t = 0)]
        rotation: u8,
    },

    /// Build an upload plan from a JPEG/PNG, and optionally send it.
    Image {
        path: PathBuf,
        /// Actually upload. Without this the plan is printed and nothing is sent.
        #[arg(long)]
        send: bool,
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=3), default_value_t = 0)]
        rotation: u8,
        /// CPU name sent by 0x15 (ASCII, truncated to 39 bytes).
        #[arg(long)]
        cpu: Option<String>,
        /// GPU name sent by 0x16 (ASCII, truncated to 39 bytes).
        #[arg(long)]
        gpu: Option<String>,
    },

    /// Build an upload plan from a GIF, and optionally send it.
    Gif {
        path: PathBuf,
        #[arg(long)]
        send: bool,
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=3), default_value_t = 0)]
        rotation: u8,
        /// CPU name sent by 0x15 (ASCII, truncated to 39 bytes).
        #[arg(long)]
        cpu: Option<String>,
        /// GPU name sent by 0x16 (ASCII, truncated to 39 bytes).
        #[arg(long)]
        gpu: Option<String>,
    },

    /// Map the command space. Dry run unless --send is given.
    #[command(long_about = EXPLORE_LONG_ABOUT)]
    Explore(ExploreArgs),

    /// Send explicit command/payload/pipe triples. Dry run unless --send.
    #[command(long_about = PROBE_LONG_ABOUT)]
    Probe(ProbeArgs),
}

#[derive(Args)]
struct ExploreArgs {
    /// Which phase to run.
    #[arg(long, value_enum, default_value_t = explore::Phase::Presence)]
    phase: explore::Phase,

    /// Actually send. Without this the plan is printed and nothing is sent.
    #[arg(long)]
    send: bool,

    /// Sweep every byte 0x00..0xFF and allow non-empty payload probes. Dangerous.
    #[arg(long)]
    unsafe_all: bool,

    /// Commands to exclude, comma-separated (e.g. 0x03,0x04,21). Applied last, even
    /// under --unsafe-all.
    #[arg(long, value_delimiter = ',', value_parser = parse_command)]
    skip: Vec<u8>,

    /// Per-probe timeout in milliseconds.
    #[arg(long, default_value_t = 750)]
    timeout_ms: u64,

    /// Directory for explore-<stamp>.json. Defaults to re/reports.
    #[arg(long)]
    out: Option<PathBuf>,

    /// Write observed commands into mystique-protocol/src/commands.rs. Requires --send.
    #[arg(long)]
    emit_constants: bool,

    /// Path to commands.rs for --emit-constants.
    #[arg(long)]
    constants_out: Option<PathBuf>,

    /// Also replay the image-upload preamble. Requires --unsafe-all.
    #[arg(long)]
    include_upload_preamble: bool,

    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=3), default_value_t = 0)]
    rotation: u8,

    /// Which pipe unknown command bytes are probed on.
    #[arg(long, value_enum, default_value_t = explore::PipeMode::Auto)]
    pipe: explore::PipeMode,
}

#[derive(Args)]
struct ProbeArgs {
    /// <cmd_hex>:<payload_hex>:<init|data>. Repeatable; order preserved.
    #[arg(
        long = "try",
        value_name = "CMD:PAYLOAD:PIPE",
        value_parser = probe::ProbeTry::parse
    )]
    tries: Vec<probe::ProbeTry>,

    /// Batch file: one `cmd_hex payload_hex pipe` per line ('#' comments).
    #[arg(long)]
    from_file: Option<PathBuf>,

    /// Actually transmit. Without it the exact 48-byte packets are printed.
    #[arg(long)]
    send: bool,

    /// Per-probe timeout in milliseconds.
    #[arg(long, default_value_t = 750)]
    timeout_ms: u64,

    /// Directory for probe-<stamp>.json. Defaults to re/reports.
    #[arg(long)]
    out: Option<PathBuf>,
}

/// A two-state flag exactly as the vendor UI spells it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum OnOff {
    On,
    Off,
}

impl OnOff {
    const fn is_on(self) -> bool {
        matches!(self, Self::On)
    }
}

fn parse_command(text: &str) -> std::result::Result<u8, String> {
    let trimmed = text.trim();
    let parsed = if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        u8::from_str_radix(hex, 16)
    } else {
        trimmed.parse::<u8>()
    };
    parsed.map_err(|_| format!("'{text}' is not a command byte (0x00..0xFF)"))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Pipes => {
            let device = open_device()?;
            print_pipes(&device)?;
        }
        Command::Status => {
            let mut link = open_link()?;
            print_status(&mut link)?;
        }
        Command::Info { show_device_id } => {
            let mut link = open_link()?;
            print_info(&mut link, show_device_id)?;
        }
        Command::Init { rotation, cpu, gpu } => {
            let mut link = open_link()?;
            let labels = machine_labels(cpu, gpu);
            run_init(&mut link, Rotation::new(rotation)?, &labels)?;
        }
        Command::Stream { seconds } => {
            let mut link = open_link()?;
            stream_synthetic(&mut link, seconds)?;
        }
        Command::Brightness { value, send } => {
            send_setting(&set_brightness(value)?, send)?;
        }
        Command::Rotate { rotation, send } => {
            println!(
                "note: rotation is ignored while the gyro is enabled, so this payload also \
                 switches the gyroscopes off."
            );
            send_setting(&rotate_with_gyro_off(Rotation::new(rotation)?), send)?;
        }
        Command::Gyro { state, send } => {
            send_setting(&set_gyro(state.is_on()), send)?;
        }
        Command::Flag0 { state, send } => {
            println!(
                "note: 0x02 byte 0 is the idle screen display, not screen enable. \
                 The hardware pass on 2026-11-18 showed it does not blank the panel; \
                 brightness 0 is the screen-off control."
            );
            send_setting(&set_idle_screen(state.is_on()), send)?;
        }
        Command::Areas {
            main,
            style,
            aux,
            send,
        } => {
            println!(
                "note: main/aux have no encoded option list (observed main 0,1,3,5 and \
                 aux 0,1,2); Style Type is 0 Normal, 1 Ring, 2 Stopwatch. Style and aux \
                 only render when main is 0 CPU Frequency."
            );
            send_setting(&set_display_areas(main, style, aux)?, send)?;
        }
        Command::MediaTiming {
            time,
            animation,
            send,
        } => {
            let timing = MediaTiming::new(time, animation)?;
            println!(
                "image switch time: {} s  animation: {}",
                timing.switch_seconds(),
                timing.animation_name()
            );
            send_setting(&set_media_timing(time, animation)?, send)?;
        }
        Command::Live { count, rotation } => {
            let mut link = open_link()?;
            run_init(
                &mut link,
                Rotation::new(rotation)?,
                &MachineLabels::default(),
            )?;
            stream_live(&mut link, count)?;
        }
        Command::Image {
            path,
            send,
            rotation,
            cpu,
            gpu,
        } => upload(
            &path,
            send,
            Rotation::new(rotation)?,
            &machine_labels(cpu, gpu),
        )?,
        Command::Gif {
            path,
            send,
            rotation,
            cpu,
            gpu,
        } => upload(
            &path,
            send,
            Rotation::new(rotation)?,
            &machine_labels(cpu, gpu),
        )?,
        Command::Explore(args) => return run_explore(args),
        Command::Probe(args) => return run_probe(args),
    }
    Ok(ExitCode::SUCCESS)
}

fn run_explore(args: ExploreArgs) -> Result<ExitCode> {
    let options = explore::ExploreOptions {
        phase: args.phase,
        send: args.send,
        unsafe_all: args.unsafe_all,
        skip: args.skip,
        timeout: Duration::from_millis(args.timeout_ms.max(1)),
        reports_dir: args.out,
        emit_constants: args.emit_constants,
        constants_out: args.constants_out,
        include_upload_preamble: args.include_upload_preamble,
        rotation: Rotation::new(args.rotation)?,
        pipe_mode: args.pipe,
    };

    match explore::run(options)? {
        explore::Outcome::Clean => Ok(ExitCode::SUCCESS),
        // Distinct exit code: a wedge needs a device restart before anything else.
        explore::Outcome::Wedged => Ok(ExitCode::from(3)),
    }
}

fn run_probe(args: ProbeArgs) -> Result<ExitCode> {
    let mut tries = args.tries;
    if let Some(path) = args.from_file.as_deref() {
        // `--try` entries keep their order; file entries follow, in file order.
        tries.extend(probe::load_file(path)?);
    }

    let options = probe::ProbeOptions {
        tries,
        send: args.send,
        timeout: Duration::from_millis(args.timeout_ms.max(1)),
        reports_dir: args.out,
    };

    match probe::run(options)? {
        explore::Outcome::Clean => Ok(ExitCode::SUCCESS),
        explore::Outcome::Wedged => Ok(ExitCode::from(3)),
    }
}

// ---------------------------------------------------------------------------
// Device helpers
// ---------------------------------------------------------------------------

fn open_device() -> Result<MystiqueDevice> {
    match MystiqueDevice::open() {
        Ok(device) => Ok(device),
        Err(DeviceError::NotFound) => {
            bail!("no MYSTIQUE device (VID 3633, PID 0009) found. Is the panel plugged in?")
        }
        Err(DeviceError::AccessDenied) => bail!(
            "access denied: another process owns the device.\n\
             Windows: stop MystiqueService and close DeepCool.exe.\n\
             Linux: install packaging/linux/99-deepcool-mystique.rules and replug."
        ),
        Err(error) => Err(error).context("opening the device"),
    }
}

fn open_link() -> Result<Link<MystiqueDevice>> {
    Ok(Link::new(open_device()?))
}

/// Fills in only the labels the caller supplied; the rest stay at the vendor's `--`.
fn machine_labels(cpu: Option<String>, gpu: Option<String>) -> MachineLabels {
    let mut labels = MachineLabels::default();
    if let Some(cpu) = cpu {
        labels.cpu = cpu;
    }
    if let Some(gpu) = gpu {
        labels.gpu = gpu;
    }
    labels
}

/// Uppercase, space-separated hex, the spelling the captures use.
fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Prints a settings packet, and writes it only when `send` is set.
///
/// Settings are full config blocks, not deltas: the whole payload is printed so the
/// operator can see every field that will be overwritten, not only the one they named.
fn send_setting(packet: &CommandPacket, send: bool) -> Result<()> {
    let bytes = packet.packet().context("building the packet")?;
    println!(
        "0x{:02X}  pipe {}  payload: {}",
        packet.code(),
        packet.pipe().label(),
        hex(&packet.payload)
    );
    println!("packet: {}", hex(&bytes));

    if !send {
        println!("\ndry run: pass --send to write it.");
        println!("the vendor service must be stopped: the panel is single-owner.");
        return Ok(());
    }

    let mut link = open_link()?;
    let response = link
        .send(packet.command, &packet.payload)
        .with_context(|| format!("sending 0x{:02X}", packet.code()))?;
    println!(
        "result: {}  reply: {}",
        response.result(),
        response.redacted_hex()
    );
    Ok(())
}

fn print_pipes(device: &MystiqueDevice) -> Result<()> {
    let summary = device.summary().context("reading descriptors")?;
    println!(
        "Device: {:04x}:{:04x}  bus {} address {}  bcdDevice {:04x}",
        summary.vendor_id, summary.product_id, summary.bus, summary.address, summary.release
    );
    println!(
        "Product: {}",
        summary
            .product
            .as_deref()
            .unwrap_or("<no string descriptor>")
    );
    println!(
        "Configurations: {}  interface descriptors: {}",
        summary.configurations, summary.interfaces
    );
    println!("Endpoints:");
    for endpoint in &summary.endpoints {
        println!(
            "  0x{:02X}  {:<3}  {:<11}  max-packet={}  interval={}",
            endpoint.address,
            endpoint.direction,
            endpoint.transfer,
            endpoint.max_packet_size,
            endpoint.interval
        );
    }

    if device.endpoints_match_documentation() {
        println!("\nLayout matches the documented 0x01/0x81 and 0x02/0x82 bulk pairs.");
    } else {
        eprintln!("\nWARNING: endpoints do not match the documented layout.");
    }
    Ok(())
}

fn print_status(link: &mut Link<MystiqueDevice>) -> Result<()> {
    let status = link.status().context("reading status")?;
    println!(
        "mode: {} (0x{:02X})  rotation: {}  result: {}",
        status.mode,
        status.raw_mode,
        status
            .rotation_degrees()
            .map(|degrees| format!("{degrees}°"))
            .unwrap_or_else(|| format!("raw 0x{:02X}", status.rotation)),
        status.result
    );
    println!("(both fields are advisory; a real capture showed 0x00 while the panel was live)");
    Ok(())
}

fn print_info(link: &mut Link<MystiqueDevice>, show_device_id: bool) -> Result<()> {
    let response = link
        .exchange_raw_on(Pipe::Init, CMD_DEVICE_INFO, &[])
        .context("requesting device info")?;
    response.check_echo(CMD_DEVICE_INFO).context("info echo")?;
    let info = DeviceInfo::parse(&response).context("parsing device info")?;

    println!("info hex:  {}", response.redacted_hex());
    println!(
        "identifier: {}",
        if show_device_id {
            info.reveal_identifier()
        } else {
            "<redacted>".to_owned()
        }
    );
    println!(
        "identifier is {}uppercase hex; padding is {}",
        if info.identifier_is_hex() { "" } else { "not " },
        if info.padding_is_zero() {
            "zero"
        } else {
            "non-zero"
        }
    );
    if !show_device_id {
        println!("pass --show-device-id to reveal it (do not publish the result)");
    }
    Ok(())
}

fn run_init(
    link: &mut Link<MystiqueDevice>,
    rotation: Rotation,
    labels: &MachineLabels,
) -> Result<()> {
    println!("Machine Info handshake, rotation {rotation}");
    println!("labels: CPU {:?}  GPU {:?}", labels.cpu, labels.gpu);
    let outcomes = link
        .init_with(rotation, labels, 0, DisplayAreas::default())
        .context("running init")?;
    for outcome in &outcomes {
        println!(
            "  0x{:02X}  result={}  {}",
            outcome.command,
            outcome.response.result(),
            outcome.response.redacted_hex()
        );
    }

    // Advisory only. Do not retry init because of this byte.
    match link.status() {
        Ok(status) => {
            println!(
                "post-init status: mode {} (0x{:02X}), rotation {}",
                status.mode,
                status.raw_mode,
                status
                    .rotation_degrees()
                    .map(|degrees| format!("{degrees}°"))
                    .unwrap_or_else(|| format!("raw 0x{:02X}", status.rotation))
            );
            if !status.is_machine_info() {
                println!(
                    "note: 0x10 did not report machine-info mode. This byte is unreliable; \
                     init is not retried."
                );
            }
        }
        Err(error) => println!("post-init status unavailable (advisory): {error}"),
    }
    Ok(())
}

fn stream_synthetic(link: &mut Link<MystiqueDevice>, seconds: u64) -> Result<()> {
    let limit = if seconds == 0 { u64::MAX } else { seconds };
    println!(
        "sending synthetic telemetry at 1 Hz ({})",
        if seconds == 0 {
            "until Ctrl+C".to_owned()
        } else {
            format!("{seconds} updates")
        }
    );

    for second in 0..limit {
        let frame = TelemetryFrame::new(
            35 + (second % 25) as u8,
            10 + (second % 80) as u8,
            40 + (second % 20) as u8,
            45 + (second % 20) as u8,
            (4200 + (second % 8) * 100) as u16,
        );
        link.telemetry(&frame).context("sending telemetry")?;
        println!(
            "  {:>5}: CPU {}C {}%  RAM {}%  GPU {}C  {} MHz",
            second + 1,
            frame.cpu_temperature_c,
            frame.cpu_usage_percent,
            frame.memory_load_percent,
            frame.gpu_temperature_c,
            frame.cpu_clock_mhz
        );
        sleep(Duration::from_secs(1));
    }
    Ok(())
}

fn stream_live(link: &mut Link<MystiqueDevice>, count: u64) -> Result<()> {
    let mut sampler = mystique_sensors::default_sampler();
    println!("sensors: {}", sampler.describe());
    let limit = if count == 0 { u64::MAX } else { count };
    let mut warned = false;

    for sent in 0..limit {
        let sample = sampler.sample();
        let frame = sample.to_frame();
        let missing = sample.missing_fields();
        link.telemetry(&frame).context("sending telemetry")?;

        let suffix = if missing.is_empty() {
            String::new()
        } else {
            format!("  [missing: {}]", missing.join(", "))
        };
        if !missing.is_empty() && !warned {
            warned = true;
        }
        println!(
            "  {:>5}: CPU {}C {}%  {} MHz  RAM {}%  GPU {}C{suffix}",
            sent + 1,
            frame.cpu_temperature_c,
            frame.cpu_usage_percent,
            frame.cpu_clock_mhz,
            frame.memory_load_percent,
            frame.gpu_temperature_c
        );
        sleep(Duration::from_secs(1));
    }
    Ok(())
}

fn upload(
    path: &std::path::Path,
    send: bool,
    rotation: Rotation,
    labels: &MachineLabels,
) -> Result<()> {
    let frames = image::load(path)?;
    // The vendor uses a fresh id per upload; two identical static JPEGs got different
    // ids in the capture. A nanosecond nonce reproduces that without adding a random
    // dependency. The device does not validate the value.
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_le_bytes();
    let content_id =
        ContentId::for_upload(frames.iter().map(|frame| frame.jpeg.as_slice()), &nonce);
    let request = UploadRequest::new(rotation, frames)
        .with_labels(labels.clone())
        .with_content_id(content_id);
    let plan = build_upload_plan(&request)?;

    println!(
        "{} frame{} from {} · content id {}",
        request.frames.len(),
        if request.frames.len() == 1 { "" } else { "s" },
        path.display(),
        request.content_id
    );
    println!("labels: CPU {:?}  GPU {:?}", labels.cpu, labels.gpu);

    let mut commands = 0usize;
    let mut headers = 0usize;
    let mut chunks = 0usize;
    let mut image_bytes = 0usize;
    for operation in &plan {
        match operation {
            Operation::Command(_) => commands += 1,
            Operation::Header(_) => headers += 1,
            Operation::Chunk(chunk) => {
                chunks += 1;
                image_bytes += chunk.len();
            }
            Operation::Finish(_) => {}
        }
    }
    println!(
        "plan: {commands} commands, {headers} frame header{}, {chunks} image chunk{} \
         ({image_bytes} bytes), 1 terminator, {} operations",
        if headers == 1 { "" } else { "s" },
        if chunks == 1 { "" } else { "s" },
        plan.len()
    );
    for operation in &plan {
        if matches!(operation, Operation::Chunk(_)) {
            continue;
        }
        println!("  {:<5} {}", operation.pipe().label(), operation.label());
    }

    if !send {
        println!("\ndry run: pass --send to upload.");
        println!(
            "warning: this switches the panel to image mode. Returning to machine-info mode \
             in the same connection is not reliably possible on this firmware."
        );
        return Ok(());
    }

    println!(
        "\nWARNING: uploading switches the panel to image mode. A 0x14 erase is sent first, \
         so a previous image is destroyed even if this upload fails."
    );
    let mut link = open_link()?;
    for (index, operation) in plan.iter().enumerate() {
        // One line per command and frame header; the thousands of 64-byte chunks are
        // progress noise, not diagnostics.
        if matches!(operation, Operation::Chunk(_)) {
            link.execute(operation)
                .with_context(|| format!("sending {}", operation.label()))?;
            continue;
        }
        match link.execute(operation) {
            Ok(Some(response)) => println!(
                "  {:>2}/{:<2} {:<40} result={}",
                index + 1,
                plan.len(),
                operation.label(),
                response.result()
            ),
            Ok(None) => println!(
                "  {:>2}/{:<2} {:<40} sent",
                index + 1,
                plan.len(),
                operation.label()
            ),
            Err(error) => {
                if let DeviceError::Rejected(result) = error {
                    println!(
                        "  {:>2}/{:<2} {:<40} rejected: {}",
                        index + 1,
                        plan.len(),
                        operation.label(),
                        result
                    );
                    if matches!(result, ResultCode::BadChecksum | ResultCode::BadMarker) {
                        bail!("the device rejected a request we built; stopping");
                    }
                } else {
                    return Err(error).context("upload failed");
                }
            }
        }
    }
    println!("upload sequence complete ({chunks} image chunks, {image_bytes} bytes)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_bytes_accept_decimal_and_hex() {
        assert_eq!(parse_command("0x0A").unwrap(), 0x0A);
        assert_eq!(parse_command("0X0a").unwrap(), 0x0A);
        assert_eq!(parse_command("10").unwrap(), 10);
        assert!(parse_command("0x100").is_err());
        assert!(parse_command("nope").is_err());
    }

    #[test]
    fn on_off_maps_to_the_payload_bit() {
        assert!(OnOff::On.is_on());
        assert!(!OnOff::Off.is_on());
    }

    #[test]
    fn settings_verbs_build_the_documented_payloads() {
        // Gyro off is the driver default, so a brightness write also clears index 1.
        assert_eq!(
            set_brightness(70).unwrap().payload,
            vec![0x01, 0x00, 0x00, 0x00, 0x46]
        );
        assert_eq!(set_gyro(true).payload, vec![0x01, 0x01, 0x00, 0x00, 0x46]);
        assert_eq!(
            set_idle_screen(false).payload,
            vec![0x00, 0x00, 0x00, 0x00, 0x46]
        );
        assert_eq!(
            rotate_with_gyro_off(Rotation::new(3).unwrap()).payload,
            vec![0x01, 0x00, 0x03, 0x00, 0x46]
        );
        assert_eq!(
            set_display_areas(5, 1, 2).unwrap().payload,
            vec![5, 1, 0, 2]
        );
        assert_eq!(set_media_timing(2, 1).unwrap().payload, vec![2, 1]);
    }

    #[test]
    fn packets_print_as_uppercase_hex() {
        assert_eq!(hex(&[0xAA, 0x2E, 0x02]), "AA 2E 02");
        assert_eq!(hex(&[]), "");
    }
}
