//! Endpoint plumbing shared by the real device and the mock.

use std::time::Duration;

use crate::error::DeviceError;

/// Everything the protocol layer needs from a USB link.
///
/// Every transfer takes an explicit [`Duration`]: there is no default, no infinite
/// wait, and no way to forget one. A hanging read is indistinguishable from a dead
/// process otherwise.
pub trait Transport {
    fn write(&mut self, endpoint: u8, data: &[u8], timeout: Duration)
        -> Result<usize, DeviceError>;

    fn read(
        &mut self,
        endpoint: u8,
        buffer: &mut [u8],
        timeout: Duration,
    ) -> Result<usize, DeviceError>;

    fn clear_halt(&mut self, endpoint: u8) -> Result<(), DeviceError>;

    fn reset(&mut self) -> Result<(), DeviceError>;
}

/// Default transfer timeout, matching the reference implementation.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(1500);

/// Default transfer timeout for one image transfer: a 64-byte header, one 64-byte
/// JPEG chunk, or the 55-byte terminator.
///
/// The capture shows thousands of small bulk transfers per frame, not one large one.
/// This value is therefore per transfer, not per frame; it is a conservative ceiling
/// rather than a measured figure.
pub const DEFAULT_FRAME_TIMEOUT: Duration = Duration::from_secs(10);
