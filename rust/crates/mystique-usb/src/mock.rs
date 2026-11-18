//! In-memory transport that behaves like the panel, for tests and dry runs.
//!
//! Two modes:
//!
//! - [`MockTransport::device`] is a small state machine: it answers like the real
//!   device, tracks rotation from `0x02`, tracks the mode byte from `0x0A`, echoes
//!   `0x00` for `0x15`/`0x16`/`0x17`/`0x0A`, and returns a synthetic identifier for
//!   `0x12`.
//! - [`MockTransport::scripted`] asserts an exact command/payload sequence and returns
//!   whatever each step says, including timeouts and stalls.
//!
//! Every write is recorded, so a test can assert what the driver actually put on the
//! wire.

use std::collections::VecDeque;
use std::time::Duration;

use mystique_protocol::commands::MODE_SWITCH_PREFIX;
use mystique_protocol::packet::checksum16;

use crate::error::{DeviceError, Transfer};
use crate::transport::Transport;

const MOCK_IDENTIFIER: &[u8; 32] = b"0123456789ABCDEF0123456789ABCDEF";

/// What the mock should do in response to one scripted write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptReply {
    /// A normal success reply.
    Ack,
    /// A normal reply with a non-zero result byte.
    Result(u8),
    /// The write succeeds but the device never answers; the next read times out.
    NoReply,
    /// The write itself times out — the wedge signature.
    WriteTimeout,
    /// The endpoint is halted on write.
    WriteStall,
    /// The device vanished.
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptStep {
    pub command: u8,
    pub payload: Vec<u8>,
    pub reply: ScriptReply,
}

impl ScriptStep {
    pub fn new(command: u8, payload: Vec<u8>, reply: ScriptReply) -> Self {
        Self {
            command,
            payload,
            reply,
        }
    }

    pub fn ack(command: u8) -> Self {
        Self::new(command, Vec::new(), ScriptReply::Ack)
    }
}

pub struct MockTransport {
    mode: u8,
    rotation: u8,
    uploading: bool,
    writes: Vec<(u8, Vec<u8>)>,
    pending: VecDeque<Vec<u8>>,
    script: Option<VecDeque<ScriptStep>>,
    wedge_command: Option<u8>,
    silent: bool,
    disconnected: bool,
}

impl MockTransport {
    /// A functioning panel in image/GIF mode, upright, with an empty upload history.
    pub fn device() -> Self {
        Self {
            mode: 0x00,
            rotation: 0,
            uploading: false,
            writes: Vec::new(),
            pending: VecDeque::new(),
            script: None,
            wedge_command: None,
            silent: false,
            disconnected: false,
        }
    }

    /// Writes succeed, reads always time out.
    pub fn silent() -> Self {
        Self {
            silent: true,
            ..Self::device()
        }
    }

    pub fn scripted(steps: Vec<ScriptStep>) -> Self {
        Self {
            script: Some(steps.into()),
            ..Self::device()
        }
    }

    /// Every write of `command` fails with a write timeout.
    pub fn wedge_on(mut self, command: u8) -> Self {
        self.wedge_command = Some(command);
        self
    }

    pub fn disconnected(mut self) -> Self {
        self.disconnected = true;
        self
    }

    /// Every write recorded so far, as `(endpoint, bytes)`.
    pub fn writes(&self) -> &[(u8, Vec<u8>)] {
        &self.writes
    }

    /// Command bytes of all 48-byte packets written, in order.
    pub fn commands(&self) -> Vec<u8> {
        self.writes
            .iter()
            .filter(|(_, data)| data.len() == 48)
            .map(|(_, data)| data[2])
            .collect()
    }

    /// Payload of the last packet sent for `command`.
    pub fn last_payload(&self, command: u8) -> Option<&[u8]> {
        self.writes
            .iter()
            .rev()
            .find(|(_, data)| data.len() == 48 && data[2] == command)
            .map(|(_, data)| &data[3..42])
    }

    pub const fn rotation(&self) -> u8 {
        self.rotation
    }

    pub const fn mode(&self) -> u8 {
        self.mode
    }

    fn reply(&mut self, echo: u8, result: u8, body: &[u8]) {
        self.pending.push_back(build_reply(echo, result, body));
    }

    fn apply_state(&mut self, command: u8, payload: &[u8]) {
        match command {
            0x02 if payload.len() >= 3 => self.rotation = payload[2].min(3),
            // The 360 sends one 0x0A payload shape everywhere, so the payload alone
            // cannot distinguish modes. The mock keeps its old convenience: 0x0A
            // enters machine-info mode until an upload begins, after which the
            // vendor's keepalive 0x0A must not leave image mode.
            0x0A if payload.starts_with(&MODE_SWITCH_PREFIX) && !self.uploading => self.mode = 0xFF,
            0x0F => {
                self.mode = 0x00;
                self.uploading = true;
            }
            _ => {}
        }
    }

    fn default_reply(&mut self, command: u8, payload: &[u8]) {
        // Verified quirk: these four echo 0x00 instead of their command byte.
        let echo = match command {
            0x15 | 0x16 | 0x17 | 0x0A => 0x00,
            other => other,
        };
        self.apply_state(command, payload);
        match command {
            0x12 => self.reply(0x12, 0x00, MOCK_IDENTIFIER),
            0x10 => {
                let body = [self.rotation, self.mode];
                self.reply(0x10, 0x00, &body);
            }
            _ => self.reply(echo, 0x00, &[]),
        }
    }
}

fn build_reply(echo: u8, result: u8, body: &[u8]) -> Vec<u8> {
    assert!(body.len() <= 38, "mock body is too long");
    let mut raw = [0u8; 48];
    raw[0] = 0x55;
    raw[1] = 0x2E;
    raw[2] = echo;
    raw[3] = result;
    raw[4..4 + body.len()].copy_from_slice(body);
    raw[42..46].copy_from_slice(b"HIDC");
    let sum = checksum16(&raw[..46]);
    raw[46..48].copy_from_slice(&sum.to_le_bytes());
    raw.to_vec()
}

impl Transport for MockTransport {
    fn write(
        &mut self,
        endpoint: u8,
        data: &[u8],
        _timeout: Duration,
    ) -> Result<usize, DeviceError> {
        if self.disconnected {
            return Err(DeviceError::Disconnected);
        }

        let command = if data.len() == 48 {
            Some(data[2])
        } else {
            None
        };
        self.writes.push((endpoint, data.to_vec()));

        if let (Some(command), Some(wedged)) = (command, self.wedge_command) {
            if command == wedged {
                return Err(DeviceError::Timeout {
                    during: Transfer::Write,
                });
            }
        }

        // Frames and the terminator are bulk blobs with no reply.
        let Some(command) = command else {
            return Ok(data.len());
        };

        let payload = &data[3..42];

        let Some(script) = self.script.as_mut() else {
            self.default_reply(command, payload);
            return Ok(data.len());
        };

        let Some(step) = script.pop_front() else {
            return Err(DeviceError::Other {
                code: -1,
                message: format!("script exhausted before command 0x{command:02X}"),
            });
        };

        if step.command != command || !payload_matches(&step.payload, payload) {
            return Err(DeviceError::Other {
                code: -1,
                message: format!(
                    "script mismatch: expected 0x{:02X} [{}], got 0x{command:02X} [{}]",
                    step.command,
                    hex(&step.payload),
                    hex(payload)
                ),
            });
        }

        let echo = match command {
            0x15 | 0x16 | 0x17 | 0x0A => 0x00,
            other => other,
        };

        match step.reply {
            ScriptReply::Ack => self.reply(echo, 0x00, &[]),
            ScriptReply::Result(result) => self.reply(echo, result, &[]),
            ScriptReply::NoReply => {}
            ScriptReply::WriteTimeout => {
                return Err(DeviceError::Timeout {
                    during: Transfer::Write,
                })
            }
            ScriptReply::WriteStall => {
                return Err(DeviceError::Stall {
                    during: Transfer::Write,
                })
            }
            ScriptReply::Disconnected => return Err(DeviceError::Disconnected),
        }

        Ok(data.len())
    }

    fn read(
        &mut self,
        _endpoint: u8,
        buffer: &mut [u8],
        _timeout: Duration,
    ) -> Result<usize, DeviceError> {
        if self.disconnected {
            return Err(DeviceError::Disconnected);
        }
        if self.silent {
            return Err(DeviceError::Timeout {
                during: Transfer::Read,
            });
        }
        let Some(reply) = self.pending.pop_front() else {
            return Err(DeviceError::Timeout {
                during: Transfer::Read,
            });
        };
        let count = reply.len().min(buffer.len());
        buffer[..count].copy_from_slice(&reply[..count]);
        Ok(count)
    }

    fn clear_halt(&mut self, _endpoint: u8) -> Result<(), DeviceError> {
        if self.disconnected {
            return Err(DeviceError::Disconnected);
        }
        Ok(())
    }

    fn reset(&mut self) -> Result<(), DeviceError> {
        if self.disconnected {
            return Err(DeviceError::Disconnected);
        }
        self.mode = 0x00;
        self.rotation = 0;
        self.uploading = false;
        self.pending.clear();
        Ok(())
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A scripted payload names the meaningful prefix; the packet pads the rest of the
/// 39-byte area with zeros.
fn payload_matches(expected: &[u8], actual: &[u8]) -> bool {
    expected.len() <= actual.len()
        && actual[..expected.len()] == *expected
        && actual[expected.len()..].iter().all(|byte| *byte == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::Link;
    use mystique_protocol::Rotation;

    #[test]
    fn acts_like_the_device_during_init() {
        let mut link = Link::new(MockTransport::device());
        assert_eq!(link.transport().mode(), 0x00);

        link.init(Rotation::ZERO).unwrap();
        assert_eq!(link.transport().mode(), 0xFF);

        let commands = link.transport().commands();
        assert_eq!(commands.len(), 13);
        assert_eq!(commands[1], 0x02);
    }

    #[test]
    fn reports_rotation_back() {
        let mut link = Link::new(MockTransport::device());
        link.init(Rotation::new(3).unwrap()).unwrap();
        assert_eq!(link.status().unwrap().rotation, 3);
    }

    #[test]
    fn a_scripted_run_detects_a_wrong_order() {
        let mut link = Link::new(MockTransport::scripted(vec![
            ScriptStep::ack(0x12),
            ScriptStep::new(0x02, vec![0x01, 0x01, 0x00, 0x00, 0x46], ScriptReply::Ack),
        ]));
        link.send(mystique_protocol::commands::Command::DeviceInfo, &[])
            .unwrap();
        link.send(
            mystique_protocol::commands::Command::DisplayConfig,
            &[0x01, 0x01, 0x00, 0x00, 0x46],
        )
        .unwrap();
        // Script is now exhausted; an extra command must fail loudly.
        assert!(link
            .send(mystique_protocol::commands::Command::InitStep03, &[0x01])
            .is_err());
    }

    #[test]
    fn a_no_reply_step_times_out_on_read() {
        let mut link = Link::new(MockTransport::scripted(vec![ScriptStep::new(
            0x10,
            vec![],
            ScriptReply::NoReply,
        )]));
        let error = link
            .send(mystique_protocol::commands::Command::Status, &[])
            .unwrap_err();
        assert_eq!(
            error,
            DeviceError::Timeout {
                during: Transfer::Read
            }
        );
    }

    #[test]
    fn a_write_stall_is_reported_as_a_stall() {
        let mut link = Link::new(MockTransport::scripted(vec![ScriptStep::new(
            0x10,
            vec![],
            ScriptReply::WriteStall,
        )]));
        let error = link
            .send(mystique_protocol::commands::Command::Status, &[])
            .unwrap_err();
        assert_eq!(
            error,
            DeviceError::Stall {
                during: Transfer::Write
            }
        );
    }

    #[test]
    fn reset_clears_the_simulated_state() {
        let mut link = Link::new(MockTransport::device());
        link.init(Rotation::new(1).unwrap()).unwrap();
        assert_eq!(link.transport().mode(), 0xFF);
        link.reset().unwrap();
        assert_eq!(link.transport().mode(), 0x00);
        assert_eq!(link.transport().rotation(), 0);
    }

    #[test]
    fn records_non_packet_writes_without_replying() {
        let mut link = Link::new(MockTransport::device());
        let mut payload = vec![0u8; 64];
        payload[..4].copy_from_slice(b"DCLd");
        link.transport_mut()
            .write(0x01, &payload, Duration::from_secs(1))
            .unwrap();
        assert_eq!(link.transport().writes().len(), 1);
        assert!(link.transport().writes()[0].1.starts_with(b"DCLd"));
    }
}
