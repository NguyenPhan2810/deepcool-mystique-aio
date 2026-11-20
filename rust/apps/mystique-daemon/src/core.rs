//! The daemon's state machine: acquire the panel, run init, stream telemetry, survive
//! loss, and give the worker something deterministic to tick.
//!
//! Everything here is synchronous and hardware-free behind [`DeviceProvider`],
//! [`Sampler`] and [`HostEnvironment`]. That is deliberate: the failure paths that
//! matter — unplug, sleep/resume, a timeout, a migrated device — are the ones that are
//! hardest to reproduce on a desk, so they are the ones the tests drive directly.
//!
//! # Ownership
//!
//! A [`Panel`] is opened once and held in `panel`. A failure drops it before scheduling
//! a retry, so the handle is never kept across a reconnect. `open_device` forces an
//! immediate attempt; `close_device` turns automatic reconnection off until the next
//! `open_device`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mystique_ipc::{codes, CommandError, DeviceState, Event, SettingsState, TelemetryStatus};
use mystique_protocol::{Clock, Command};
use mystique_sensors::Sampler;

use crate::environment::{HostEnvironment, ServiceSnapshot};
use crate::panel::{DeviceProvider, Panel};
use crate::state::{frame_values, Settings, StreamStatus};
use crate::upload::PendingUpload;

/// Timings and startup intent. The CLI sets `interval`; tests shrink every duration.
#[derive(Debug, Clone)]
pub struct CoreConfig {
    /// Telemetry period. 1 s by default.
    pub interval: Duration,
    /// First retry delay after a device fault.
    pub retry_base: Duration,
    /// Retry ceiling.
    pub retry_max: Duration,
    /// A timeout is treated as a possible wedge, so its retry is at least this long.
    pub wedge_retry: Duration,
    /// Start with the stream on. The daemon's whole job is to keep the panel fed.
    pub initial_stream: bool,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(1),
            retry_base: Duration::from_secs(1),
            retry_max: Duration::from_secs(30),
            wedge_retry: Duration::from_secs(30),
            initial_stream: true,
        }
    }
}

pub struct Core {
    provider: Box<dyn DeviceProvider>,
    sampler: Box<dyn Sampler>,
    environment: Box<dyn HostEnvironment>,
    config: CoreConfig,

    pub(crate) panel: Option<Box<dyn Panel>>,
    pub(crate) settings: Settings,
    pub(crate) stream: StreamStatus,
    pub(crate) last_error: Option<String>,
    pub(crate) pending_upload: Option<PendingUpload>,
    /// False after `close_device`, so a retry cannot silently undo an explicit close.
    pub(crate) auto_reconnect: bool,
    /// Where successful settings changes are written. `None` in tests and when the
    /// per-user directory cannot be found; the daemon then behaves as it did before
    /// persistence existed.
    pub(crate) settings_path: Option<PathBuf>,

    present: bool,
    service: ServiceSnapshot,
    next_tick: Instant,
    next_retry: Instant,
    /// When the next `0x0A` clock payload is due. Independent of the stream: the panel's
    /// clock must keep correct even when telemetry is stopped.
    next_clock: Instant,
    attempt: u32,
}

impl Core {
    pub fn new(
        provider: Box<dyn DeviceProvider>,
        sampler: Box<dyn Sampler>,
        environment: Box<dyn HostEnvironment>,
        config: CoreConfig,
        settings: Settings,
        now: Instant,
    ) -> Self {
        let stream = StreamStatus::new(config.interval, config.initial_stream);
        let mut environment = environment;
        let service = environment.service();
        let present = environment.device_present();
        Self {
            provider,
            sampler,
            environment,
            config,
            panel: None,
            settings,
            stream,
            last_error: None,
            pending_upload: None,
            auto_reconnect: true,
            settings_path: None,
            present,
            service,
            next_tick: now,
            next_retry: now,
            next_clock: now,
            attempt: 0,
        }
    }

    pub fn is_open(&self) -> bool {
        self.panel.is_some()
    }

    /// Enables the settings file. Called once, before the worker takes ownership; the
    /// startup settings themselves are already loaded by the caller.
    pub fn with_settings_path(mut self, path: PathBuf) -> Self {
        self.settings_path = Some(path);
        self
    }

    /// Writes the current settings after a successful change. A failure is reported and
    /// dropped: the device command already succeeded and must not be reported as failed.
    pub(crate) fn persist_settings(&self) {
        let Some(path) = self.settings_path.as_deref() else {
            return;
        };
        if let Err(message) = crate::settings_file::save(path, &self.settings) {
            eprintln!("mystique-daemon: could not save the settings: {message}");
        }
    }

    pub fn is_stream_running(&self) -> bool {
        self.stream.running()
    }

    pub fn settings(&self) -> SettingsState {
        self.settings.snapshot()
    }

    pub fn telemetry_status(&self) -> TelemetryStatus {
        self.stream.status(self.is_open())
    }

    /// The cached state. Cheap: no USB enumeration and no `sc.exe`.
    pub fn state(&self) -> DeviceState {
        DeviceState {
            device_open: self.is_open(),
            device_present: self.present,
            service_running: self.service.running,
            service_supported: self.service.supported,
            service_output: self.service.output.clone(),
            last_error: self.last_error.clone(),
            rotation: self.settings.last_rotation,
            settings: self.settings.snapshot(),
            telemetry: Some(self.stream.status(self.is_open())),
        }
    }

    pub fn state_event(&self) -> Event {
        Event::state(self.state())
    }

    /// Re-queries the two host facts a client may care about. Called by `status` and
    /// `device_state`, not by the 1 Hz loop.
    pub fn refresh_environment(&mut self) {
        self.present = self.environment.device_present();
        self.service = self.environment.service();
    }

    /// How long the worker may sleep before this core needs a tick. `None` means nothing
    /// is scheduled: the worker can block until a request arrives.
    pub fn next_wait(&self, now: Instant) -> Option<Duration> {
        let mut deadline: Option<Instant> = None;
        if self.panel.is_none() && self.auto_reconnect {
            deadline = Some(self.next_retry);
        }
        if self.panel.is_some() {
            deadline = Some(match deadline {
                Some(current) => current.min(self.next_clock),
                None => self.next_clock,
            });
        }
        if self.panel.is_some() && self.stream.running() {
            deadline = Some(match deadline {
                Some(current) => current.min(self.next_tick),
                None => self.next_tick,
            });
        }
        deadline.map(|value| value.saturating_duration_since(now))
    }

    /// One clock tick: acquire if due, then send telemetry if due. Never panics and
    /// never blocks on anything but the device itself.
    pub fn tick(&mut self, now: Instant) -> Vec<Event> {
        let mut events = Vec::new();
        if self.panel.is_none() && self.auto_reconnect && now >= self.next_retry {
            self.attempt_acquire(now, &mut events);
        }
        if self.panel.is_some() && now >= self.next_clock {
            self.sync_clock(now);
        }
        if self.panel.is_some() && self.stream.running() && now >= self.next_tick {
            self.tick_telemetry(now, &mut events);
        }
        events
    }

    /// Sends the panel's clock. `0x0A` is not a heartbeat: it carries the local date and
    /// time, and the vendor app sends it about once a second. A failure here is silent —
    /// a wrong clock is annoying, a broken stream is not worth it.
    fn sync_clock(&mut self, now: Instant) {
        let Some(panel) = self.panel.as_mut() else {
            return;
        };
        let payload = local_clock().payload();
        let _ = panel.send(Command::Clock, &payload);
        self.next_clock = now + Duration::from_secs(1);
    }

    fn attempt_acquire(&mut self, now: Instant, events: &mut Vec<Event>) {
        match self.open_once() {
            Ok(()) => {
                self.note_open_success(now);
                events.push(self.state_event());
            }
            Err(error) => {
                self.note_open_failure(&error, now);
                events.push(self.state_event());
            }
        }
    }

    /// Opens and initialises once. The panel is only stored after init succeeds, so a
    /// half-initialised handle can never be mistaken for a working one.
    pub(crate) fn open_once(&mut self) -> Result<(), CommandError> {
        let mut panel = self.provider.acquire()?;
        panel
            .init(
                self.settings.display,
                self.settings.recording,
                &self.settings.labels,
                local_clock(),
                self.settings.areas,
            )
            .map_err(crate::error::from_device)?;
        self.panel = Some(panel);
        Ok(())
    }

    pub(crate) fn note_open_success(&mut self, now: Instant) {
        self.attempt = 0;
        self.last_error = None;
        self.present = true;
        self.next_tick = now;
    }

    pub(crate) fn note_open_failure(&mut self, error: &CommandError, now: Instant) {
        self.last_error = Some(error.message.clone());
        self.schedule_retry(now, &error.code);
        self.present = self.environment.device_present();
    }

    pub(crate) fn schedule_retry(&mut self, now: Instant, code: &str) {
        self.attempt = self.attempt.saturating_add(1);
        let mut delay = backoff(self.config.retry_base, self.config.retry_max, self.attempt);
        if code == codes::TIMEOUT {
            delay = delay.max(self.config.wedge_retry);
        }
        self.next_retry = now.checked_add(delay).unwrap_or(now);
    }

    pub(crate) fn note_stream_started(&mut self, now: Instant) {
        self.next_tick = now;
    }

    /// Changes the telemetry period in place and applies it on the next tick. The
    /// rate is a host property: nothing about it is sent to the panel.
    pub(crate) fn set_stream_interval(&mut self, interval: Duration, now: Instant) {
        self.config.interval = interval;
        self.stream.set_interval(interval);
        self.note_stream_started(now);
    }

    fn tick_telemetry(&mut self, now: Instant, events: &mut Vec<Event>) {
        let sample_started = Instant::now();
        let sample = self.sampler.sample();
        let sample_cost = sample_started.elapsed();
        let source = self.sampler.describe().to_owned();
        let (cpu_fan, pump) = self.settings.fan_io.route(&sample);
        let values = frame_values(&sample, &source, cpu_fan, pump);
        let wire = sample
            .to_frame()
            .with_fans(pump.unwrap_or(0), cpu_fan.unwrap_or(0));

        let transfer_started = Instant::now();
        let sent = match self.panel.as_mut() {
            Some(panel) => panel.telemetry(&wire),
            None => return,
        };
        let transfer_cost = transfer_started.elapsed();
        match sent {
            Ok(()) => {
                let had_error = self.last_error.take().is_some();
                self.stream.note_frame(values.clone());
                self.stream.note_timing(now, sample_cost, transfer_cost);
                self.next_tick = advance(self.next_tick, now, self.config.interval);
                events.push(Event::telemetry(values));
                if had_error {
                    events.push(self.state_event());
                }
            }
            Err(failure) => {
                // A transport fault means the link is gone. A rejection means the
                // device answered; keep the handle and try the next frame on it.
                let error = crate::error::from_device(failure);
                self.stream.note_error(error.message.clone());
                self.last_error = Some(error.message.clone());
                if crate::error::should_reacquire(&error.code) {
                    self.panel = None;
                    self.schedule_retry(now, &error.code);
                } else {
                    self.next_tick = advance(self.next_tick, now, self.config.interval);
                }
                events.push(self.state_event());
            }
        }
    }

    /// One host sample for an immediate redraw frame, from the same sampler the stream
    /// uses, so the numbers match what the next streamed frame would carry.
    pub(crate) fn sample_frame(&mut self) -> mystique_protocol::TelemetryFrame {
        let sample = self.sampler.sample();
        let (cpu_fan, pump) = self.settings.fan_io.route(&sample);
        sample
            .to_frame()
            .with_fans(pump.unwrap_or(0), cpu_fan.unwrap_or(0))
    }

    /// Stops the stream and leaves the panel dark, deliberately. A process that is
    /// exiting cannot keep numbers fresh, so it does not leave stale ones lit.
    pub fn shutdown(&mut self) -> Vec<Event> {
        self.stream.stop();
        if let Some(panel) = self.panel.as_mut() {
            if self.settings.display.brightness() > 0 {
                if let Ok(dark) = self.settings.display.with_brightness(0) {
                    let _ = panel.send(Command::DisplayConfig, &dark.payload());
                }
            }
        }
        self.panel = None;
        self.pending_upload = None;
        self.last_error = None;
        vec![self.state_event()]
    }
}

/// The next deadline, kept on the original phase so work time does not shorten the
/// period. If the work overruns by more than one interval, the schedule resets instead
/// of trying to send a backlog back to back.
pub fn advance(deadline: Instant, now: Instant, interval: Duration) -> Instant {
    match deadline.checked_add(interval) {
        Some(next) if next > now => next,
        _ => now.checked_add(interval).unwrap_or(now),
    }
}

/// The local wall clock as the protocol crate's [`Clock`]. Local, not UTC: the vendor
/// app sends local time and the panel draws it verbatim.
///
/// This is the daemon's only conversion. The init handshake, the upload preamble and the
/// once-a-second `0x0A` all use it, so the panel cannot be told two different dates.
pub(crate) fn local_clock() -> Clock {
    use chrono::{Datelike, Local, Timelike};
    let now = Local::now();
    Clock::new(
        now.month() as u8,
        now.day() as u8,
        now.hour() as u8,
        now.minute() as u8,
        now.second() as u8,
    )
}

/// Exponential backoff, capped: base, 2×base, 4×base, … `max`.
pub fn backoff(base: Duration, max: Duration, attempts: u32) -> Duration {
    if attempts == 0 {
        return base.min(max);
    }
    let shift = (attempts - 1).min(6);
    let factor = 1u32 << shift;
    base.saturating_mul(factor).min(max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_period_keeps_its_phase_instead_of_absorbing_work() {
        let start = Instant::now();
        let interval = Duration::from_millis(33);
        // A 4 ms frame must not turn 33 ms into 37 ms.
        assert_eq!(
            advance(start, start + Duration::from_millis(4), interval),
            start + interval
        );
        // A late tick resets the schedule; it never fires a backlog back to back.
        let late = start + Duration::from_millis(50);
        assert_eq!(advance(start, late, interval), late + interval);
    }

    #[test]
    fn backoff_grows_and_caps() {
        let base = Duration::from_millis(10);
        let max = Duration::from_millis(80);
        assert_eq!(backoff(base, max, 0), base);
        assert_eq!(backoff(base, max, 1), base);
        assert_eq!(backoff(base, max, 2), Duration::from_millis(20));
        assert_eq!(backoff(base, max, 3), Duration::from_millis(40));
        assert_eq!(backoff(base, max, 4), Duration::from_millis(80));
        assert_eq!(backoff(base, max, 99), max);
    }

    #[test]
    fn a_zero_base_never_panics() {
        assert_eq!(
            backoff(Duration::ZERO, Duration::from_secs(1), 9),
            Duration::ZERO
        );
    }
}
