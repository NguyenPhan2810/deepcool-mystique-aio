//! The protocol layer wired to a [`Transport`].

use std::thread::sleep;
use std::time::Duration;

use mystique_protocol::{
    build_packet,
    commands::{pipe_for, Command},
    init::machine_info_sequence_full,
    parse_status, DeviceInfo, DisplayAreas, DisplayConfig, FinishPacket, MachineLabels, Operation,
    PanelStatus, ProtocolError, RecordingConfig, Response, ResultCode, Rotation, TelemetryFrame,
    IMAGE_CHUNK_LEN, READ_BUFFER_LEN,
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
            0,
            DisplayAreas::default(),
        )
    }

    /// The handshake with CPU/GPU labels, a starting `0x0A` counter and the display
    /// areas to carry in `0x04`.
    pub fn init_with(
        &mut self,
        rotation: Rotation,
        labels: &MachineLabels,
        heartbeat: u16,
        areas: DisplayAreas,
    ) -> Result<Vec<InitOutcome>, DeviceError> {
        let display = DisplayConfig::default()
            .with_gyro(true)
            .with_rotation(rotation);
        self.init_with_config(
            display,
            RecordingConfig::default(),
            labels,
            heartbeat,
            areas,
        )
    }

    /// The handshake with the caller's complete `0x02` display state and `0x05`
    /// recording options. `0x02` is absolute state, so a reconnect must not reset the
    /// session's idle screen or RGB sync to the crate defaults.
    pub fn init_with_config(
        &mut self,
        display: DisplayConfig,
        recording: RecordingConfig,
        labels: &MachineLabels,
        heartbeat: u16,
        areas: DisplayAreas,
    ) -> Result<Vec<InitOutcome>, DeviceError> {
        let steps = machine_info_sequence_full(display, recording, labels, heartbeat, areas);
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
mod tests {
    use super::*;
    use crate::mock::{MockTransport, ScriptReply, ScriptStep};

    fn link() -> Link<MockTransport> {
        Link::new(MockTransport::device())
    }

    fn jpeg() -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        bytes.extend_from_slice(b"JFIF\0");
        bytes.extend_from_slice(&[0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00]);
        bytes.extend_from_slice(&[0x42; 128]);
        bytes.extend_from_slice(&[0xFF, 0xD9]);
        bytes
    }

    #[test]
    fn init_runs_thirteen_steps_and_enters_machine_info() {
        let mut link = link();
        let outcomes = link.init(Rotation::ZERO).unwrap();
        assert_eq!(outcomes.len(), 13);
        assert_eq!(
            outcomes.iter().map(|o| o.command).collect::<Vec<_>>(),
            vec![0x12, 0x02, 0x03, 0x04, 0x07, 0x08, 0x05, 0x0B, 0x06, 0x15, 0x16, 0x17, 0x0A]
        );
        // 0x15/0x16/0x17/0x0A echo 0x00 on the real device.
        assert_eq!(outcomes[9].response.command_echo(), 0x00);
        assert_eq!(outcomes[12].response.command_echo(), 0x00);

        let status = link.status().unwrap();
        assert!(status.is_machine_info());
        assert_eq!(status.rotation, 0);
    }

    #[test]
    fn init_sets_the_requested_rotation() {
        let mut link = link();
        link.init(Rotation::new(2).unwrap()).unwrap();
        assert_eq!(link.transport().rotation(), 2);
        assert_eq!(link.status().unwrap().rotation, 2);
    }

    #[test]
    fn status_is_advisory_but_parsed() {
        let mut link = link();
        link.init(Rotation::ZERO).unwrap();
        let status = link.status().unwrap();
        assert_eq!(status.raw_mode, 0xFF);
    }

    #[test]
    fn info_is_redacted() {
        let mut link = link();
        let info = link.info().unwrap();
        assert!(info.identifier_is_hex());
        assert!(!format!("{info:?}").contains("0123456789ABCDEF"));
    }

    #[test]
    fn request_11_returns_the_raw_body_and_it_is_zero_filled() {
        let mut link = link();
        let body = link.request_11_body().unwrap();
        assert_eq!(body.len(), 38);
        assert!(body.iter().all(|byte| *byte == 0x00));
        // Sent on the init OUT endpoint, not the data one.
        let writes = link.transport().writes();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].0, 0x01);
        assert_eq!(writes[0].1[2], 0x11);
    }

    #[test]
    fn an_empty_mode_switch_is_refused_before_it_reaches_the_wire() {
        let mut link = link();
        let error = link.send(Command::ModeSwitch, &[]).unwrap_err();
        assert_eq!(
            error,
            DeviceError::Protocol(ProtocolError::PayloadRequired { command: 0x0A })
        );
        assert!(link.transport().writes().is_empty());
    }

    #[test]
    fn a_device_rejection_maps_to_rejected_not_protocol() {
        let mut link = Link::new(MockTransport::scripted(vec![ScriptStep::new(
            0x10,
            vec![],
            ScriptReply::Result(0x02),
        )]));
        let error = link.send(Command::Status, &[]).unwrap_err();
        assert_eq!(error, DeviceError::Rejected(ResultCode::BadChecksum));
    }

    #[test]
    fn a_read_timeout_is_not_a_wedge() {
        let mut link = Link::new(MockTransport::silent());
        let error = link.send(Command::Status, &[]).unwrap_err();
        assert_eq!(
            error,
            DeviceError::Timeout {
                during: Transfer::Read
            }
        );
        assert!(!error.is_wedge());
    }

    #[test]
    fn a_write_timeout_is_a_wedge() {
        let mut link = Link::new(MockTransport::device().wedge_on(0x10));
        let error = link.send(Command::Status, &[]).unwrap_err();
        assert!(error.is_wedge());
    }

    #[test]
    fn a_script_mismatch_is_reported_clearly() {
        let mut link = Link::new(MockTransport::scripted(vec![ScriptStep::new(
            0x12,
            vec![],
            ScriptReply::Ack,
        )]));
        let error = link.send(Command::Status, &[]).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("script mismatch"), "{message}");
    }

    #[test]
    fn a_frame_is_written_as_one_header_transfer_then_64_byte_chunks() {
        use mystique_protocol::{build_upload_plan, Operation, UploadFrame, UploadRequest};
        let jpeg = jpeg();
        assert_eq!(jpeg.len(), 150);
        let request = UploadRequest::new(Rotation::ZERO, vec![UploadFrame::new(jpeg.clone())]);
        let plan = build_upload_plan(&request).unwrap();

        let mut link = link();
        for operation in &plan {
            if matches!(operation, Operation::Header(_) | Operation::Chunk(_)) {
                link.execute(operation).unwrap();
            }
        }

        let writes = link.transport().writes();
        assert_eq!(writes.len(), 4, "one header plus three JPEG chunks");
        assert_eq!(writes[0].0, 0x01);
        assert_eq!(writes[0].1.len(), 64);
        assert_eq!(&writes[0].1[..4], b"DCLd");
        assert_eq!(
            writes[1..]
                .iter()
                .map(|(_, data)| data.len())
                .collect::<Vec<_>>(),
            vec![64, 64, 22]
        );
        assert!(writes[1..].iter().all(|(_, data)| data.len() <= 64));

        let reassembled: Vec<u8> = writes[1..]
            .iter()
            .flat_map(|(_, data)| data.iter().copied())
            .collect();
        assert_eq!(reassembled, jpeg);
    }

    #[test]
    fn an_oversized_or_empty_chunk_is_refused_before_the_wire() {
        let mut link = link();
        let error = link.write_chunk(&[0u8; 65]).unwrap_err();
        assert_eq!(
            error,
            DeviceError::Protocol(ProtocolError::ImageChunkTooLong {
                actual: 65,
                max: 64
            })
        );
        let error = link.write_chunk(&[]).unwrap_err();
        assert_eq!(error, DeviceError::Protocol(ProtocolError::EmptyImageChunk));
        assert!(link.transport().writes().is_empty());
    }

    #[test]
    fn a_full_upload_plan_executes_end_to_end() {
        use mystique_protocol::{build_upload_plan, Operation, UploadFrame, UploadRequest};
        let request = UploadRequest::new(
            Rotation::ZERO,
            vec![UploadFrame::new(jpeg()).with_delay(50)],
        );
        let plan = build_upload_plan(&request).unwrap();

        let mut link = link();
        let mut headers = 0;
        let mut chunks = 0;
        for operation in &plan {
            match operation {
                Operation::Header(_) => headers += 1,
                Operation::Chunk(_) => chunks += 1,
                _ => {}
            }
            link.execute(operation)
                .unwrap_or_else(|error| panic!("{} failed: {error}", operation.label()));
        }
        assert_eq!(headers, 1);
        assert_eq!(chunks, 3);

        let writes = link.transport().writes();
        let frame_writes: Vec<&(u8, Vec<u8>)> = writes
            .iter()
            .filter(|(_, data)| data.starts_with(b"DCLd") || data.starts_with(b"dcldfinish"))
            .collect();
        // One header (64) and one terminator (55), never one combined buffer.
        assert_eq!(frame_writes.len(), 2);
        assert_eq!(frame_writes[0].1.len(), 64);
        assert_eq!(frame_writes[1].1.len(), 55);
        assert!(!writes
            .iter()
            .any(|(_, data)| data.len() > 64 && !data.starts_with(b"\xaa\x2e")));
    }
}
