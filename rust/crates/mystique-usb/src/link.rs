//! The protocol layer wired to a [`Transport`].

use std::thread::sleep;
use std::time::Duration;

use mystique_protocol::{
    build_packet,
    commands::{pipe_for, Command},
    init::machine_info_sequence_full,
    parse_status, Clock, DeviceInfo, DisplayAreas, DisplayConfig, FinishPacket, MachineLabels,
    Operation, PanelStatus, ProtocolError, RecordingConfig, Response, ResultCode, Rotation,
    TelemetryFrame, IMAGE_CHUNK_LEN, READ_BUFFER_LEN,
};

use crate::error::{DeviceError, Transfer};
use crate::transport::{Transport, DEFAULT_FRAME_TIMEOUT, DEFAULT_TIMEOUT};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkOptions {
    pub timeout: Duration,
    pub frame_timeout: Duration,
}

impl Default for LinkOptions {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
            frame_timeout: DEFAULT_FRAME_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InitOutcome {
    pub command: u8,
    pub response: Response,
}

/// A request/response link over any [`Transport`].
///
/// The real device answers strictly request/response: both IN endpoints are silent
/// without a write (`re/fixtures/10-no-unsolicited-traffic.json`). A read is therefore
/// always preceded by a write in this type.
pub struct Link<T: Transport> {
    transport: T,
    options: LinkOptions,
}

impl<T: Transport> Link<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            options: LinkOptions::default(),
        }
    }

    pub fn with_options(transport: T, options: LinkOptions) -> Self {
        Self { transport, options }
    }

    pub const fn options(&self) -> LinkOptions {
        self.options
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    pub fn into_inner(self) -> T {
        self.transport
    }

    /// Writes a validated 48-byte packet. The only writer in the crate; callers
    /// cannot produce a packet with a bad sync byte or checksum.
    pub fn write_packet(
        &mut self,
        pipe: mystique_protocol::Pipe,
        command: u8,
        payload: &[u8],
        timeout: Duration,
    ) -> Result<(), DeviceError> {
        let packet = build_packet(command, payload)?;
        self.transport
            .write(pipe.out_endpoint(), &packet, timeout)
            .map(|_| ())
    }

    /// Reads one reply and validates the envelope.
    pub fn read_response(
        &mut self,
        pipe: mystique_protocol::Pipe,
        timeout: Duration,
    ) -> Result<Response, DeviceError> {
        let mut buffer = [0u8; READ_BUFFER_LEN];
        let read = self
            .transport
            .read(pipe.in_endpoint(), &mut buffer, timeout)?;
        if read == 0 {
            return Err(DeviceError::Protocol(ProtocolError::Truncated {
                actual: 0,
                need: 48,
            }));
        }
        Ok(Response::parse(&buffer[..read])?)
    }

    /// Writes and reads without judging the result byte or the echo.
    ///
    /// This is what the explorer needs: the echo and the result are data.
    pub fn exchange_raw_on(
        &mut self,
        pipe: mystique_protocol::Pipe,
        command: u8,
        payload: &[u8],
    ) -> Result<Response, DeviceError> {
        self.write_packet(pipe, command, payload, self.options.timeout)?;
        self.read_response(pipe, self.options.timeout)
    }

    pub fn exchange_raw(&mut self, command: u8, payload: &[u8]) -> Result<Response, DeviceError> {
        self.exchange_raw_on(pipe_for(command), command, payload)
    }

    /// Strict exchange used by [`Link::send`]. Private on purpose: the public typed
    /// path is the one that carries the pipe and payload rules.
    fn send_on(
        &mut self,
        pipe: mystique_protocol::Pipe,
        command: u8,
        payload: &[u8],
    ) -> Result<Response, DeviceError> {
        let response = self.exchange_raw_on(pipe, command, payload)?;
        check_response(&response, command)?;
        Ok(response)
    }

    /// Strict exchange for a typed command. The pipe and the payload requirement
    /// come from [`Command`], so the caller cannot route it to the wrong endpoint.
    pub fn send(&mut self, command: Command, payload: &[u8]) -> Result<Response, DeviceError> {
        if command.requires_payload() && payload.is_empty() {
            return Err(DeviceError::Protocol(ProtocolError::PayloadRequired {
                command: command.code(),
            }));
        }
        self.send_on(command.pipe(), command.code(), payload)
    }

    /// `0x10`. Rotation and mode are returned even when the device reports a bad
    /// checksum, and the mode byte is advisory — callers log it, never gate on it.
    pub fn status(&mut self) -> Result<PanelStatus, DeviceError> {
        let response = self.exchange_raw_on(Command::Status.pipe(), Command::Status.code(), &[])?;
        response.check_echo(Command::Status.code())?;
        Ok(parse_status(&response))
    }

    /// `0x12`, identifier redacted by the returned type.
    pub fn info(&mut self) -> Result<DeviceInfo, DeviceError> {
        let response =
            self.exchange_raw_on(Command::DeviceInfo.pipe(), Command::DeviceInfo.code(), &[])?;
        Ok(DeviceInfo::parse(&response)?)
    }

    /// `0x11`. Sends the prior-art "serial request" and returns the raw 38-byte body.
    ///
    /// This is deliberately **not** a serial. The sweep found the body all zeros on
    /// this 360 (`re/findings/11-sweep-results.md`), so there is nothing to parse and
    /// nothing is named. `0x12` remains the only identity source.
    pub fn request_11_body(&mut self) -> Result<Vec<u8>, DeviceError> {
        let response =
            self.exchange_raw_on(Command::Request11.pipe(), Command::Request11.code(), &[])?;
        response.check_echo(Command::Request11.code())?;
        Ok(response.body().to_vec())
    }

    /// `0x01`, 39-byte telemetry payload.
    pub fn telemetry(&mut self, frame: &TelemetryFrame) -> Result<Response, DeviceError> {
        self.send(Command::Telemetry, &frame.encode())
    }

    /// The 13-command handshake with the documented settle delays and the default
    /// display areas (main `00` CPU Frequency, aux `00` Voltage Mode).
    ///
    /// The result does not depend on the post-init status byte. A real capture showed
    /// `0x00` while the panel was live in Machine Info mode, so gating on `0xFF` would
    /// be wrong (`re/findings/06-image-upload-protocol.md`).
    pub fn init(&mut self, rotation: Rotation) -> Result<Vec<InitOutcome>, DeviceError> {
        self.init_with(
            rotation,
            &MachineLabels::default(),
            Clock::zero(),
            DisplayAreas::default(),
        )
    }

    /// The handshake with CPU/GPU labels, the clock reading for `0x0A` and the display
    /// areas to carry in `0x04`.
    ///
    /// A caller with no clock source passes [`Clock::zero`]; the daemon passes the live
    /// local time so the panel starts on the right date instead of the capture's.
    pub fn init_with(
        &mut self,
        rotation: Rotation,
        labels: &MachineLabels,
        clock: Clock,
        areas: DisplayAreas,
    ) -> Result<Vec<InitOutcome>, DeviceError> {
        let display = DisplayConfig::default()
            .with_gyro(true)
            .with_rotation(rotation);
        self.init_with_config(display, RecordingConfig::default(), labels, clock, areas)
    }

    /// The handshake with the caller's complete `0x02` display state and `0x05`
    /// recording options. `0x02` is absolute state, so a reconnect must not reset the
    /// session's idle screen or RGB sync to the crate defaults.
    ///
    /// `0x0A` carries `clock`; see [`Clock`].
    pub fn init_with_config(
        &mut self,
        display: DisplayConfig,
        recording: RecordingConfig,
        labels: &MachineLabels,
        clock: Clock,
        areas: DisplayAreas,
    ) -> Result<Vec<InitOutcome>, DeviceError> {
        let steps = machine_info_sequence_full(display, recording, labels, clock, areas);
        let mut outcomes = Vec::with_capacity(steps.len());
        for step in steps {
            let response = self.send(step.command, &step.payload)?;
            outcomes.push(InitOutcome {
                command: step.command.code(),
                response,
            });
            sleep(step.settle);
        }
        Ok(outcomes)
    }

    /// One image transfer. Callers must already have split the frame.
    fn write_image_transfer(&mut self, data: &[u8]) -> Result<(), DeviceError> {
        self.transport
            .write(
                mystique_protocol::Pipe::Init.out_endpoint(),
                data,
                self.options.frame_timeout,
            )
            .map(|_| ())
    }

    /// The 64-byte `DCLd` header, as its own transfer.
    ///
    /// The type makes the size explicit; this method deliberately takes the whole
    /// header and nothing else, so a caller cannot append JPEG bytes to it.
    pub fn write_header(
        &mut self,
        header: &[u8; mystique_protocol::DCLD_HEADER_LEN],
    ) -> Result<(), DeviceError> {
        self.write_image_transfer(header)
    }

    /// One JPEG chunk, at most 64 bytes, as its own transfer.
    pub fn write_chunk(&mut self, chunk: &[u8]) -> Result<(), DeviceError> {
        if chunk.is_empty() {
            return Err(ProtocolError::EmptyImageChunk.into());
        }
        if chunk.len() > IMAGE_CHUNK_LEN {
            return Err(ProtocolError::ImageChunkTooLong {
                actual: chunk.len(),
                max: IMAGE_CHUNK_LEN,
            }
            .into());
        }
        self.write_image_transfer(chunk)
    }

    pub fn write_finish(&mut self, finish: &FinishPacket) -> Result<(), DeviceError> {
        self.transport
            .write(
                mystique_protocol::Pipe::Init.out_endpoint(),
                finish.bytes(),
                self.options.timeout,
            )
            .map(|_| ())
    }

    /// Executes one plan operation. Commands get the strict request/response path;
    /// each header, chunk and terminator is a single write of its own.
    pub fn execute(&mut self, operation: &Operation) -> Result<Option<Response>, DeviceError> {
        match operation {
            Operation::Command(command) => {
                let response = self.send(command.command, &command.payload)?;
                if !command.settle.is_zero() {
                    sleep(command.settle);
                }
                Ok(Some(response))
            }
            Operation::Header(header) => {
                self.write_header(header)?;
                Ok(None)
            }
            Operation::Chunk(chunk) => {
                self.write_chunk(chunk)?;
                Ok(None)
            }
            Operation::Finish(finish) => {
                self.write_finish(finish)?;
                Ok(None)
            }
        }
    }

    /// Reads once and discards the result, including a timeout.
    ///
    /// Used only after a read timeout during exploration, so a late reply is not
    /// attributed to the next command.
    pub fn drain(&mut self, timeout: Duration) -> Option<Response> {
        let mut buffer = [0u8; READ_BUFFER_LEN];
        for pipe in [mystique_protocol::Pipe::Data, mystique_protocol::Pipe::Init] {
            match self
                .transport
                .read(pipe.in_endpoint(), &mut buffer, timeout)
            {
                Ok(read) if read > 0 => return Response::parse(&buffer[..read]).ok(),
                Ok(_) => continue,
                Err(_) => continue,
            }
        }
        None
    }

    /// Recovery primitives, exercised by the CLI and available to a future daemon.
    pub fn clear_halt(&mut self, endpoint: u8) -> Result<(), DeviceError> {
        self.transport.clear_halt(endpoint)
    }

    pub fn reset(&mut self) -> Result<(), DeviceError> {
        self.transport.reset()
    }
}

fn check_response(response: &Response, command: u8) -> Result<(), DeviceError> {
    match response.require_success(command) {
        Ok(()) => Ok(()),
        Err(ProtocolError::DeviceRejected { result, .. }) => Err(DeviceError::Rejected(result)),
        Err(other) => Err(DeviceError::Protocol(other)),
    }
}

/// Convenience: map a raw result byte to the enum for callers that only have a byte.
pub fn result_code(raw: u8) -> ResultCode {
    ResultCode::from_raw(raw)
}

/// Timeout marker used by the explorer when a read produced no data. Kept here so the
/// CLI and any future daemon describe the same thing the same way.
pub const fn read_timeout() -> DeviceError {
    DeviceError::Timeout {
        during: Transfer::Read,
    }
}

#[cfg(test)]
#[cfg(test)]
mod tests;
