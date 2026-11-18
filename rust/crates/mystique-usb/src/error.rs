use mystique_protocol::{ProtocolError, ResultCode};
use thiserror::Error;

/// Which direction a transfer was going when it failed. Needed because a **write**
/// timeout is the wedge signature on this device, while a **read** timeout is an
/// ordinary "this command byte is not implemented" result during exploration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Transfer {
    Read,
    Write,
    Control,
}

impl Transfer {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Control => "control",
        }
    }
}

impl std::fmt::Display for Transfer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DeviceError {
    #[error("no MYSTIQUE device (VID 0x3633, PID 0x0009) found")]
    NotFound,

    #[error("access denied: another process owns the device interface")]
    AccessDenied,

    #[error("transfer timed out during {during}")]
    Timeout { during: Transfer },

    #[error("endpoint stalled during {during}")]
    Stall { during: Transfer },

    /// Windows `ERROR_BAD_COMMAND` (Win32 22), as libusb reports it.
    ///
    /// Seen after exploratory probing left the panel in an odd state: the next
    /// telemetry write was refused until the service was restarted once
    /// (`re/findings/11-sweep-results.md`). This is recoverable, not a wedge and not
    /// a disconnect — retry, and tell the operator to restart the service once.
    #[error("device answered with ERROR_BAD_COMMAND (Win32 22) during {during}; recoverable")]
    BadCommand { during: Transfer },

    #[error("device disconnected")]
    Disconnected,

    #[error("protocol error: {0}")]
    Protocol(#[from] ProtocolError),

    #[error("device rejected the command: {0}")]
    Rejected(ResultCode),

    #[error("usb error {code}: {message}")]
    Other { code: i32, message: String },
}

impl DeviceError {
    /// The signature of the failure that wedged the unit for ~24 minutes: the bulk
    /// OUT endpoint stops accepting writes. Stop the sweep; do not retry into a
    /// second one (`re/findings/09-protocol-mapping-plan.md`, safety rule 5).
    pub fn is_wedge(&self) -> bool {
        matches!(
            self,
            Self::Timeout {
                during: Transfer::Write
            }
        )
    }

    /// True when retrying without operator action cannot help.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Disconnected
                | Self::AccessDenied
                | Self::Timeout {
                    during: Transfer::Write
                }
        )
    }

    /// True when the driver should keep going (or retry) rather than stop the run.
    ///
    /// `ERROR_BAD_COMMAND` belongs here: the post-sweep anomaly showed the device
    /// refusing normal telemetry for a while without the link being dead, and a
    /// single service restart cleared it.
    pub fn is_recoverable(&self) -> bool {
        !self.is_terminal()
    }

    pub fn during(&self) -> Option<Transfer> {
        match self {
            Self::Timeout { during } | Self::Stall { during } | Self::BadCommand { during } => {
                Some(*during)
            }
            _ => None,
        }
    }
}

impl From<rusb::Error> for DeviceError {
    fn from(error: rusb::Error) -> Self {
        Self::from_rusb(error, Transfer::Control)
    }
}

impl DeviceError {
    pub(crate) fn from_rusb(error: rusb::Error, during: Transfer) -> Self {
        match error {
            rusb::Error::Access | rusb::Error::Busy => Self::AccessDenied,
            rusb::Error::NoDevice => Self::Disconnected,
            rusb::Error::NotFound => Self::NotFound,
            rusb::Error::Timeout => Self::Timeout { during },
            rusb::Error::Pipe => Self::Stall { during },
            // libusb collapses Win32 ERROR_BAD_COMMAND into its generic I/O error.
            // Treat a bare I/O failure as recoverable; a vanished device reports
            // NoDevice, which stays terminal.
            rusb::Error::Io | rusb::Error::Other => Self::BadCommand { during },
            other => Self::Other {
                code: other as i32,
                message: other.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_write_timeout_is_a_wedge() {
        assert!(DeviceError::Timeout {
            during: Transfer::Write
        }
        .is_wedge());
        assert!(!DeviceError::Timeout {
            during: Transfer::Read
        }
        .is_wedge());
        assert!(!DeviceError::Stall {
            during: Transfer::Write
        }
        .is_wedge());
    }

    #[test]
    fn a_bad_command_is_recoverable_not_terminal() {
        let error = DeviceError::from_rusb(rusb::Error::Io, Transfer::Write);
        assert_eq!(
            error,
            DeviceError::BadCommand {
                during: Transfer::Write
            }
        );
        assert!(!error.is_wedge());
        assert!(!error.is_terminal());
        assert!(error.is_recoverable());
        assert_eq!(error.during(), Some(Transfer::Write));
    }

    #[test]
    fn a_vanished_device_stays_terminal() {
        let error = DeviceError::from_rusb(rusb::Error::NoDevice, Transfer::Read);
        assert_eq!(error, DeviceError::Disconnected);
        assert!(error.is_terminal());
        assert!(!error.is_recoverable());
    }

    #[test]
    fn maps_the_libusb_errors_we_care_about() {
        assert_eq!(
            DeviceError::from_rusb(rusb::Error::Access, Transfer::Control),
            DeviceError::AccessDenied
        );
        assert_eq!(
            DeviceError::from_rusb(rusb::Error::Timeout, Transfer::Read),
            DeviceError::Timeout {
                during: Transfer::Read
            }
        );
        assert_eq!(
            DeviceError::from_rusb(rusb::Error::Pipe, Transfer::Write),
            DeviceError::Stall {
                during: Transfer::Write
            }
        );
        assert_eq!(
            DeviceError::from_rusb(rusb::Error::NoDevice, Transfer::Read),
            DeviceError::Disconnected
        );
    }
}
