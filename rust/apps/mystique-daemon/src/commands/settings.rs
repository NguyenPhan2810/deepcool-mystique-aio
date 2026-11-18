//! The documented settings commands plus screen power. Each one records the exact
//! packet it wrote and returns the session snapshot that follows it.

use std::time::Instant;

use mystique_ipc::{args, CommandError, PacketRecord, Request, Response};
use mystique_protocol::{
    erase_stored_images, set_mode, Command, CommandPacket, DisplayAreas, DisplayTarget,
    MediaTiming, RecordingConfig, Rotation, SETTLE_STEP,
};

use super::{failed, ok, parse};
use crate::core::Core;
use crate::error;
use crate::state::{validate_label, FanIo, FanSource};

impl Core {
    pub(super) fn command_set_brightness(&mut self, request: &Request, now: Instant) -> Response {
        let args = match parse::<args::Brightness>(request) {
            Ok(value) => value,
            Err(error) => return failed(request, error),
        };
        let config = match self.settings.display.with_brightness(args.brightness) {
            Ok(value) => value,
            Err(error) => return failed(request, error::from_protocol(error)),
        };
        let records = match self.write_packets(vec![config.packet()], now) {
            Ok(records) => records,
            Err(error) => return failed(request, error),
        };
        self.settings.display = config;
        if args.brightness > 0 {
            self.settings.screen_restore = Some(args.brightness);
        }
        let message = if args.brightness == 0 {
            format!(
                "brightness 0 sent: the panel is dark. Screen On restores {}%",
                self.settings.screen_restore_brightness()
            )
        } else {
            format!("brightness {} written to 0x02 byte 4", args.brightness)
        };
        ok(request, &self.outcome(records, message))
    }

    pub(super) fn command_set_rotation(&mut self, request: &Request, now: Instant) -> Response {
        let args = match parse::<args::Rotation>(request) {
            Ok(value) => value,
            Err(error) => return failed(request, error),
        };
        let rotation = match Rotation::new(args.rotation) {
            Ok(value) => value,
            Err(error) => return failed(request, error::from_protocol(error)),
        };
        // The panel ignores rotation while the gyro is enabled; clear it in the same
        // payload, exactly as the UI does.
        let config = self
            .settings
            .display
            .with_gyro(false)
            .with_rotation(rotation);
        let records = match self.write_packets(vec![config.packet()], now) {
            Ok(records) => records,
            Err(error) => return failed(request, error),
        };
        self.settings.display = config;
        self.settings.last_rotation = Some(rotation.raw());
        ok(
            request,
            &self.outcome(
                records,
                format!(
                    "rotation {} degrees sent with gyroscopes off in the same 0x02 payload",
                    rotation.degrees()
                ),
            ),
        )
    }

    pub(super) fn command_set_gyro(&mut self, request: &Request, now: Instant) -> Response {
        let args = match parse::<args::Enabled>(request) {
            Ok(value) => value,
            Err(error) => return failed(request, error),
        };
        let config = self.settings.display.with_gyro(args.enabled);
        let records = match self.write_packets(vec![config.packet()], now) {
            Ok(records) => records,
            Err(error) => return failed(request, error),
        };
        self.settings.display = config;
        let message = if args.enabled {
            "gyroscopes allowed (0x02 byte 1); rotation is ignored while this is on"
        } else {
            "gyroscopes off (0x02 byte 1); rotation can now take effect"
        };
        ok(request, &self.outcome(records, message.to_owned()))
    }

    /// `0x02[0]`: idle screen display. This is not screen power and never blanks the
    /// panel; brightness 0 is the screen-off control.
    pub(super) fn command_set_idle_screen(&mut self, request: &Request, now: Instant) -> Response {
        let args = match parse::<args::Enabled>(request) {
            Ok(value) => value,
            Err(error) => return failed(request, error),
        };
        let config = self.settings.display.with_idle_screen(args.enabled);
        let records = match self.write_packets(vec![config.packet()], now) {
            Ok(records) => records,
            Err(error) => return failed(request, error),
        };
        self.settings.display = config;
        let message = if args.enabled {
            "idle screen display enabled (0x02 byte 0); the panel stays lit"
        } else {
            "idle screen display disabled (0x02 byte 0); this is not screen power"
        };
        ok(request, &self.outcome(records, message.to_owned()))
    }

    /// `0x02[3]`: RGB light effect sync. The LCD firmware defers the ring colour to the
    /// motherboard; the temperature warning belongs to the LED subsystem and is absent.
    pub(super) fn command_set_rgb_sync(&mut self, request: &Request, now: Instant) -> Response {
        let args = match parse::<args::Enabled>(request) {
            Ok(value) => value,
            Err(error) => return failed(request, error),
        };
        let config = self.settings.display.with_rgb_sync(args.enabled);
        let records = match self.write_packets(vec![config.packet()], now) {
            Ok(records) => records,
            Err(error) => return failed(request, error),
        };
        self.settings.display = config;
        let message = if args.enabled {
            "RGB sync on (0x02 byte 3); the ring colour follows the motherboard"
        } else {
            "RGB sync off (0x02 byte 3); the panel controls its own colour"
        };
        ok(request, &self.outcome(records, message.to_owned()))
    }

    /// `0x05`: the two Recording-mode graph options. The graphs only draw with data, so
    /// the write is followed by one immediate `0x01` frame, exactly like `0x04`.
    /// The two RPM dials are routed on the host: the panel is told one number per dial
    /// and nothing else. Changing the routing therefore sends no setting packet, only
    /// one immediate frame so the dials redraw with the new source.
    pub(super) fn command_set_fan_io(&mut self, request: &Request, now: Instant) -> Response {
        let args = match parse::<args::FanIo>(request) {
            Ok(value) => value,
            Err(error) => return failed(request, error),
        };
        let (Some(cpu), Some(pump)) = (FanSource::parse(&args.cpu), FanSource::parse(&args.pump))
        else {
            return failed(
                request,
                CommandError::range(format!(
                    "fan sources must be cpu, pump or none; got \"{}\" and \"{}\"",
                    args.cpu, args.pump
                )),
            );
        };
        self.settings.fan_io = FanIo { cpu, pump };

        let record = match self.redraw_frame(now) {
            Ok(record) => record,
            Err(error) => return failed(request, error),
        };
        ok(
            request,
            &self.outcome(
                vec![record],
                format!(
                    "fan routing: first dial {}, second dial {}; one frame pushed to redraw",
                    cpu.as_str(),
                    pump.as_str()
                ),
            ),
        )
    }

    pub(super) fn command_set_mode(&mut self, request: &Request, now: Instant) -> Response {
        let args = match parse::<args::Mode>(request) {
            Ok(value) => value,
            Err(error) => return failed(request, error),
        };
        let target = match DisplayTarget::from_raw(args.mode) {
            Some(target) => target,
            None => {
                return failed(
                    request,
                    CommandError::range(format!(
                        "mode {} is not 1 Monitor, 2 Media or 3 Recording",
                        args.mode
                    )),
                )
            }
        };
        let records = match self.write_packets(vec![set_mode(target)], now) {
            Ok(records) => records,
            Err(error) => return failed(request, error),
        };
        ok(
            request,
            &self.outcome(
                records,
                format!(
                    "0x03 = {} sent; stored images still hold the panel until erase",
                    target.label()
                ),
            ),
        )
    }

    pub(super) fn command_set_recording(&mut self, request: &Request, now: Instant) -> Response {
        let args = match parse::<args::Recording>(request) {
            Ok(value) => value,
            Err(error) => return failed(request, error),
        };
        let recording = RecordingConfig::new(args.cpu_clock, args.cpu_temp);
        let mut records = match self.write_packets(vec![recording.packet()], now) {
            Ok(records) => records,
            Err(error) => return failed(request, error),
        };
        self.settings.recording = recording;

        let redraw_note = match self.redraw_frame(now) {
            Ok(record) => {
                records.push(record);
                "one telemetry frame pushed to redraw"
            }
            Err(error) => {
                records.extend(error.packets);
                "the redraw frame could not be sent"
            }
        };

        ok(
            request,
            &self.outcome(
                records,
                format!(
                    "recording options sent (0x05): CPU frequency {}, CPU temperature {}; \
                     {redraw_note}",
                    if args.cpu_clock { "on" } else { "off" },
                    if args.cpu_temp { "on" } else { "off" },
                ),
            ),
        )
    }

    /// `enabled = false` is brightness 0; `true` restores the last lit brightness.
    pub(super) fn command_set_screen(&mut self, request: &Request, now: Instant) -> Response {
        let args = match parse::<args::Enabled>(request) {
            Ok(value) => value,
            Err(error) => return failed(request, error),
        };
        let target = if args.enabled {
            self.settings.screen_restore_brightness()
        } else {
            0
        };
        let config = match self.settings.display.with_brightness(target) {
            Ok(value) => value,
            Err(error) => return failed(request, error::from_protocol(error)),
        };
        let records = match self.write_packets(vec![config.packet()], now) {
            Ok(records) => records,
            Err(error) => return failed(request, error),
        };
        self.settings.display = config;
        if args.enabled {
            self.settings.screen_restore = Some(target);
        }
        let message = if args.enabled {
            format!("brightness {target} restored; the panel is lit")
        } else {
            format!(
                "brightness 0 sent: the panel is dark. Screen On restores {}%",
                self.settings.screen_restore_brightness()
            )
        };
        ok(request, &self.outcome(records, message))
    }

    pub(super) fn command_set_areas(&mut self, request: &Request, now: Instant) -> Response {
        let args = match parse::<args::Areas>(request) {
            Ok(value) => value,
            Err(error) => return failed(request, error),
        };
        let areas = match DisplayAreas::new(args.main, args.style, args.aux) {
            Ok(value) => value,
            Err(error) => return failed(request, error::from_protocol(error)),
        };
        let mut records = match self.write_packets(vec![areas.packet()], now) {
            Ok(records) => records,
            Err(error) => return failed(request, error),
        };
        self.settings.areas = areas;

        // `0x04` redraws the layout, but a style byte only appears once a `0x01` frame
        // arrives. Push one immediately so the owner sees the change without waiting for
        // the stream — the same behaviour the desktop app had before it became a client.
        // The redraw is a convenience: a failed frame never fails the layout write, and
        // it does not touch the stream's counters or cadence.
        let redraw_note = match self.redraw_frame(now) {
            Ok(record) => {
                records.push(record);
                "one telemetry frame pushed to redraw"
            }
            Err(error) => {
                records.extend(error.packets);
                "the redraw frame could not be sent"
            }
        };

        ok(
            request,
            &self.outcome(
                records,
                format!(
                    "display areas sent: main selector {0}, style type {1}, aux selector {2} \
                     (0x04); {redraw_note}",
                    args.main, args.style, args.aux
                ),
            ),
        )
    }

    /// One immediate `0x01` frame, sampled from the daemon's own sampler.
    fn redraw_frame(&mut self, now: Instant) -> Result<PacketRecord, CommandError> {
        let frame = self.sample_frame();
        let packet = CommandPacket::new(Command::Telemetry, frame.encode().to_vec(), SETTLE_STEP);
        self.write_packet(packet, now)
    }

    pub(super) fn command_set_media_timing(&mut self, request: &Request, now: Instant) -> Response {
        let args = match parse::<args::MediaTiming>(request) {
            Ok(value) => value,
            Err(error) => return failed(request, error),
        };
        let timing = match MediaTiming::new(args.time, args.animation) {
            Ok(value) => value,
            Err(error) => return failed(request, error::from_protocol(error)),
        };
        let records = match self.write_packets(vec![timing.packet()], now) {
            Ok(records) => records,
            Err(error) => return failed(request, error),
        };
        self.settings.media_time = args.time;
        self.settings.media_animation = args.animation;
        ok(
            request,
            &self.outcome(
                records,
                format!(
                    "media timing sent: {} s, {} (0x07)",
                    timing.switch_seconds(),
                    timing.animation_name()
                ),
            ),
        )
    }

    pub(super) fn command_set_labels(&mut self, request: &Request, now: Instant) -> Response {
        let args = match parse::<args::Labels>(request) {
            Ok(value) => value,
            Err(error) => return failed(request, error),
        };
        if args.cpu.is_none() && args.gpu.is_none() {
            return failed(
                request,
                CommandError::range("provide a CPU label, a GPU label, or both"),
            );
        }
        let mut labels = self.settings.labels.clone();
        if let Some(value) = args.cpu {
            if let Err(message) = validate_label(&value, "CPU") {
                return failed(request, CommandError::range(message));
            }
            labels.cpu = value;
        }
        if let Some(value) = args.gpu {
            if let Err(message) = validate_label(&value, "GPU") {
                return failed(request, CommandError::range(message));
            }
            labels.gpu = value;
        }
        let packets = vec![
            CommandPacket::new(Command::InitStep15, labels.cpu_payload(), SETTLE_STEP),
            CommandPacket::new(Command::InitStep16, labels.gpu_payload(), SETTLE_STEP),
        ];
        let records = match self.write_packets(packets, now) {
            Ok(records) => records,
            Err(error) => return failed(request, error),
        };
        self.settings.labels = labels;
        ok(
            request,
            &self.outcome(
                records,
                "CPU and GPU labels written with 0x15 and 0x16".to_owned(),
            ),
        )
    }

    /// `0x14` — erase the stored image or GIF. Empty payload, init pipe, sent alone.
    ///
    /// This is the verified route back from media to machine info, and it is
    /// destructive. The panel has no read-back, so the message says what was sent
    /// rather than claiming what is displayed.
    pub(super) fn command_erase_images(&mut self, request: &Request, now: Instant) -> Response {
        let records = match self.write_packets(vec![erase_stored_images()], now) {
            Ok(records) => records,
            Err(error) => return failed(request, error),
        };
        ok(
            request,
            &self.outcome(
                records,
                "0x14 erase sent with an empty payload on the init pipe. Stored content \
                 erased; the panel returns to machine info. The daemon cannot read back \
                 what remains."
                    .to_owned(),
            ),
        )
    }
}
