//! libusb transport and device lifecycle for the DeepCool MYSTIQUE AIO LCD.
//!
//! The device is USB class `0xFF` with vendor-specific bulk endpoints, so there is
//! no HID or serial layer: libusb (via `rusb`) is the whole transport.
//!
//! `rusb` links against a **vendored** libusb built by `libusb1-sys`, so no system
//! libusb is required on Windows or Linux. See `rust/README.md` for the exact
//! resolution used on this machine.

#![forbid(unsafe_code)]

pub mod device;
pub mod discover;
pub mod error;
pub mod link;
pub mod mock;
pub mod transport;

pub use device::MystiqueDevice;
pub use discover::{DeviceSummary, EndpointSummary};
pub use error::{DeviceError, Transfer};
pub use link::{InitOutcome, Link, LinkOptions};
pub use mock::{MockTransport, ScriptReply, ScriptStep};
pub use transport::{Transport, DEFAULT_FRAME_TIMEOUT, DEFAULT_TIMEOUT};
