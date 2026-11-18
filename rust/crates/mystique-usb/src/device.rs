//! Owned, exclusive access to the panel.

use std::time::Duration;

use mystique_protocol::{Pipe, READ_BUFFER_LEN};
use rusb::{DeviceHandle, GlobalContext};

use crate::discover::{DeviceSummary, INTERFACE};
use crate::error::{DeviceError, Transfer};
use crate::transport::Transport;

/// A claimed interface plus its handle.
///
/// Dropping this releases the interface. The device is single-owner in practice: a
/// second open gets `ERROR_ACCESS_DENIED` on Windows, and on Linux libusb's claim is
/// exclusive once a kernel driver is detached. `Drop` is best-effort and never panics.
pub struct MystiqueDevice {
    handle: DeviceHandle<GlobalContext>,
}

impl MystiqueDevice {
    /// Opens interface 0, detaching a kernel driver on Linux if one is bound.
    pub fn open() -> Result<Self, DeviceError> {
        let device = crate::discover::find()?;
        let handle = device.open().map_err(DeviceError::from)?;

        // Linux-only concept. Not supported on Windows/WinUSB; the error is expected
        // there and irrelevant because WinUSB already owns the interface.
        let _ = handle.set_auto_detach_kernel_driver(true);

        handle
            .claim_interface(INTERFACE)
            .map_err(DeviceError::from)?;

        Ok(Self { handle })
    }

    pub fn summary(&self) -> Result<DeviceSummary, DeviceError> {
        crate::discover::summarize(&self.handle.device())
    }

    /// Re-enumerates the four endpoints and asserts the documented layout.
    ///
    /// Returns `false` (rather than an error) when the endpoints differ, so a caller
    /// can print a clear warning without refusing to talk to the device.
    pub fn endpoints_match_documentation(&self) -> bool {
        const EXPECTED: [(u8, bool); 4] =
            [(0x01, false), (0x81, true), (0x02, false), (0x82, true)];
        match self.summary() {
            Ok(summary) => EXPECTED.iter().all(|(address, _)| {
                summary
                    .endpoints
                    .iter()
                    .any(|endpoint| endpoint.address == *address && endpoint.transfer == "bulk")
            }),
            Err(_) => false,
        }
    }

    fn read_into(
        &mut self,
        endpoint: u8,
        buffer: &mut [u8],
        timeout: Duration,
    ) -> Result<usize, DeviceError> {
        self.handle
            .read_bulk(endpoint, buffer, timeout)
            .map_err(|error| DeviceError::from_rusb(error, Transfer::Read))
    }
}

impl Transport for MystiqueDevice {
    fn write(
        &mut self,
        endpoint: u8,
        data: &[u8],
        timeout: Duration,
    ) -> Result<usize, DeviceError> {
        self.handle
            .write_bulk(endpoint, data, timeout)
            .map_err(|error| DeviceError::from_rusb(error, Transfer::Write))
    }

    fn read(
        &mut self,
        endpoint: u8,
        buffer: &mut [u8],
        timeout: Duration,
    ) -> Result<usize, DeviceError> {
        self.read_into(endpoint, buffer, timeout)
    }

    fn clear_halt(&mut self, endpoint: u8) -> Result<(), DeviceError> {
        self.handle
            .clear_halt(endpoint)
            .map_err(|error| DeviceError::from_rusb(error, Transfer::Control))
    }

    /// Device reset, then re-claim. libusb documents that a reset can invalidate
    /// interface claims on some platforms, so the claim is restored here.
    fn reset(&mut self) -> Result<(), DeviceError> {
        self.handle
            .reset()
            .map_err(|error| DeviceError::from_rusb(error, Transfer::Control))?;
        let _ = self.handle.release_interface(INTERFACE);
        self.handle
            .claim_interface(INTERFACE)
            .map_err(DeviceError::from)?;
        Ok(())
    }
}

impl Drop for MystiqueDevice {
    fn drop(&mut self) {
        let _ = self.handle.release_interface(INTERFACE);
    }
}

/// Endpoint numbers, re-exported so callers do not repeat them.
pub const INIT_OUT: u8 = Pipe::Init.out_endpoint();
pub const INIT_IN: u8 = Pipe::Init.in_endpoint();
pub const DATA_OUT: u8 = Pipe::Data.out_endpoint();
pub const DATA_IN: u8 = Pipe::Data.in_endpoint();

/// The read buffer size the reference implementation offers. 48 bytes arrive.
pub const DEVICE_READ_BUFFER: usize = READ_BUFFER_LEN;
