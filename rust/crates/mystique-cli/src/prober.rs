//! One raw probe: send a packet, read a reply, record what happened.
//!
//! Shared by `explore` and `probe` so both produce byte-identical entries for the
//! analysis script. Nothing here judges a result code: the entry is data.

use std::time::{Duration, Instant};

use mystique_protocol::Pipe;
use mystique_usb::{DeviceError, Link, Transfer, Transport};

use crate::report::{Entry, StatusSnapshot};

pub struct ProbeOutcome {
    pub entry: Entry,
    /// Set only when continuing would be pointless: a write timeout (wedge) or a
    /// disconnect. Read timeouts are data and never appear here.
    pub fatal: Option<DeviceError>,
}

pub struct Prober<T: Transport> {
    link: Link<T>,
    index: usize,
}

impl<T: Transport> Prober<T> {
    pub fn new(link: Link<T>) -> Self {
        Self { link, index: 0 }
    }

    pub fn probe(
        &mut self,
        pipe: Pipe,
        command: u8,
        payload: &[u8],
        label: Option<String>,
    ) -> ProbeOutcome {
        let mut entry = Entry::new(self.index, command, pipe.label(), payload);
        entry.label = label;
        self.index += 1;

        let started = Instant::now();
        let result = self.link.exchange_raw_on(pipe, command, payload);
        entry.elapsed_ms = started.elapsed().as_millis() as u64;

        match result {
            Ok(response) => {
                entry.replied = true;
                entry.echo = Some(response.command_echo());
                entry.result_code = Some(response.result().as_raw());
                entry.reply_len = Some(response.wire_len());
                entry.reply_hex_redacted = Some(response.redacted_hex());
                entry.outcome = "reply".to_owned();
                ProbeOutcome { entry, fatal: None }
            }
            Err(error) => {
                let mut fatal = None;
                entry.outcome = match &error {
                    DeviceError::Timeout {
                        during: Transfer::Write,
                    } => "wedge",
                    DeviceError::Timeout { .. } => "timeout",
                    DeviceError::Stall { .. } => "stall",
                    DeviceError::Disconnected => "disconnected",
                    _ => "error",
                }
                .to_owned();
                entry.error = Some(error.to_string());

                if matches!(
                    error,
                    DeviceError::Stall {
                        during: Transfer::Write
                    }
                ) {
                    // A halted endpoint is recoverable; clear it and keep going.
                    if let Err(clear) = self.link.clear_halt(pipe.out_endpoint()) {
                        entry.error = Some(format!("{error}; clear_halt failed: {clear}"));
                        fatal = Some(clear);
                    }
                } else if error.is_wedge() || matches!(error, DeviceError::Disconnected) {
                    fatal = Some(error);
                }

                if entry.outcome == "timeout" {
                    // A late reply must not be attributed to the next command.
                    let _ = self.link.drain(Duration::from_millis(20));
                }

                ProbeOutcome { entry, fatal }
            }
        }
    }

    /// `0x10` is advisory: a read timeout means "no status right now", not a failure.
    /// A write timeout is still a wedge and stops the run.
    pub fn status_snapshot(&mut self) -> Result<Option<StatusSnapshot>, DeviceError> {
        match self.link.status() {
            Ok(status) => Ok(Some(StatusSnapshot::from(status))),
            Err(error) if error.is_wedge() || matches!(error, DeviceError::Disconnected) => {
                Err(error)
            }
            Err(_) => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mystique_usb::mock::MockTransport;

    #[test]
    fn a_reply_records_every_required_field() {
        let mut prober = Prober::new(Link::new(MockTransport::device()));
        let outcome = prober.probe(Pipe::Data, 0x10, &[], None);
        assert!(outcome.entry.replied);
        assert_eq!(outcome.entry.echo, Some(0x10));
        assert_eq!(outcome.entry.result_code, Some(0x00));
        assert_eq!(outcome.entry.reply_len, Some(48));
        assert!(outcome.entry.reply_hex_redacted.is_some());
        assert!(outcome.fatal.is_none());
    }

    #[test]
    fn a_timeout_is_data_not_a_failure() {
        let mut prober = Prober::new(Link::new(MockTransport::silent()));
        let outcome = prober.probe(Pipe::Data, 0x0F, &[], None);
        assert!(!outcome.entry.replied);
        assert_eq!(outcome.entry.outcome, "timeout");
        assert!(outcome.fatal.is_none());
    }

    #[test]
    fn a_write_timeout_is_fatal() {
        let mut prober = Prober::new(Link::new(MockTransport::device().wedge_on(0x0F)));
        let outcome = prober.probe(Pipe::Init, 0x0F, &[], None);
        assert_eq!(outcome.entry.outcome, "wedge");
        assert!(outcome.fatal.unwrap().is_wedge());
    }
}
