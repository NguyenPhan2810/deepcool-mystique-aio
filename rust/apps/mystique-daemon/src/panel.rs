//! The device boundary.
//!
//! [`Panel`] is everything the daemon needs from a connected panel. The real
//! implementation wraps `Link<MystiqueDevice>`; tests provide an in-memory panel so the
//! state machine — acquire, init, stream, lose, retry — is exercised with no hardware.
//!
//! A `Panel` is held for as long as the link is healthy. Dropping it releases the USB
//! interface, which is exactly what device loss must do before a retry.

use mystique_ipc::{CommandError, DeviceInfo};
use mystique_protocol::{
    Clock, Command, DisplayAreas, DisplayConfig, MachineLabels, Operation, PanelStatus, Pipe,
    RecordingConfig, Response, TelemetryFrame,
};
use mystique_usb::{DeviceError, DeviceSummary, Link, MystiqueDevice};

/// One connected panel. Every method is a real transfer on the real implementation.
pub trait Panel: Send {
    /// The 13-command machine-info handshake, with the documented settle delays and the
    /// clock reading that goes into `0x0A`.
    fn init(
        &mut self,
        display: DisplayConfig,
        recording: RecordingConfig,
        labels: &MachineLabels,
        clock: Clock,
        areas: DisplayAreas,
    ) -> Result<(), DeviceError>;
    /// One `0x01` frame.
    fn telemetry(&mut self, frame: &TelemetryFrame) -> Result<(), DeviceError>;
    /// A strict typed exchange for a settings command.
    fn send(&mut self, command: Command, payload: &[u8]) -> Result<Response, DeviceError>;
    /// A raw exchange on a caller-chosen pipe. Used only by the raw packet panel; the
    /// result code is data, never a success gate.
    fn send_raw(
        &mut self,
        pipe: Pipe,
        command: u8,
        payload: &[u8],
    ) -> Result<Response, DeviceError>;
    /// `0x10`, advisory.
    fn status(&mut self) -> Result<PanelStatus, DeviceError>;
    /// One upload-plan operation: a command, a frame header, a chunk or the terminator.
    fn execute(&mut self, operation: &Operation) -> Result<Option<Response>, DeviceError>;
    /// Descriptor summary, redaction-safe, for `open_device`.
    fn info(&self) -> Option<DeviceInfo>;
    /// False when the four documented endpoints are not all present.
    fn endpoints_ok(&self) -> bool;
}

/// Opens the panel. Called once at startup and again after every loss.
pub trait DeviceProvider: Send {
    fn acquire(&mut self) -> Result<Box<dyn Panel>, CommandError>;
}

/// The real provider: enumerate VID/PID `3633:0009` and claim interface 0.
pub struct RealProvider;

impl DeviceProvider for RealProvider {
    fn acquire(&mut self) -> Result<Box<dyn Panel>, CommandError> {
        let device = MystiqueDevice::open().map_err(crate::error::from_device)?;
        let endpoints_ok = device.endpoints_match_documentation();
        let info = device.summary().ok().map(|summary| device_info(&summary));
        Ok(Box::new(RealPanel {
            link: Link::new(device),
            info,
            endpoints_ok,
        }))
    }
}

struct RealPanel {
    link: Link<MystiqueDevice>,
    info: Option<DeviceInfo>,
    endpoints_ok: bool,
}

impl Panel for RealPanel {
    fn init(
        &mut self,
        display: DisplayConfig,
        recording: RecordingConfig,
        labels: &MachineLabels,
        clock: Clock,
        areas: DisplayAreas,
    ) -> Result<(), DeviceError> {
        self.link
            .init_with_config(display, recording, labels, clock, areas)
            .map(|_| ())
    }

    fn telemetry(&mut self, frame: &TelemetryFrame) -> Result<(), DeviceError> {
        self.link.telemetry(frame).map(|_| ())
    }

    fn send(&mut self, command: Command, payload: &[u8]) -> Result<Response, DeviceError> {
        self.link.send(command, payload)
    }

    fn send_raw(
        &mut self,
        pipe: Pipe,
        command: u8,
        payload: &[u8],
    ) -> Result<Response, DeviceError> {
        self.link.exchange_raw_on(pipe, command, payload)
    }

    fn status(&mut self) -> Result<PanelStatus, DeviceError> {
        self.link.status()
    }

    fn execute(&mut self, operation: &Operation) -> Result<Option<Response>, DeviceError> {
        self.link.execute(operation)
    }

    fn info(&self) -> Option<DeviceInfo> {
        self.info.clone()
    }

    fn endpoints_ok(&self) -> bool {
        self.endpoints_ok
    }
}

/// The descriptor summary as the UI shows it. No serial and no device identifier.
pub(crate) fn device_info(summary: &DeviceSummary) -> DeviceInfo {
    DeviceInfo {
        product: summary.product.clone(),
        bus: summary.bus,
        address: summary.address,
        release: format!("0x{:04X}", summary.release),
        endpoints: summary
            .endpoints
            .iter()
            .map(|endpoint| {
                format!(
                    "0x{:02X} {} {} max {}B",
                    endpoint.address,
                    endpoint.direction,
                    endpoint.transfer,
                    endpoint.max_packet_size
                )
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mystique_usb::EndpointSummary;

    fn summary() -> DeviceSummary {
        DeviceSummary {
            bus: 1,
            address: 5,
            vendor_id: 0x3633,
            product_id: 0x0009,
            release: 0x0246,
            product: Some("MYSTIQUE".to_owned()),
            manufacturer: Some("DeepCool".to_owned()),
            configurations: 1,
            interfaces: 1,
            endpoints: vec![
                EndpointSummary {
                    address: 0x01,
                    direction: "out",
                    transfer: "bulk",
                    max_packet_size: 512,
                    interval: 0,
                },
                EndpointSummary {
                    address: 0x81,
                    direction: "in",
                    transfer: "bulk",
                    max_packet_size: 512,
                    interval: 0,
                },
            ],
        }
    }

    #[test]
    fn the_info_dto_is_the_ui_shape_and_carries_no_identifier() {
        let info = device_info(&summary());
        assert_eq!(info.product.as_deref(), Some("MYSTIQUE"));
        assert_eq!(info.bus, 1);
        assert_eq!(info.address, 5);
        assert_eq!(info.release, "0x0246");
        assert_eq!(
            info.endpoints,
            ["0x01 out bulk max 512B", "0x81 in bulk max 512B"]
        );
        assert!(!format!("{info:?}").contains("0123456789ABCDEF"));
    }
}
