//! Host-side facts the daemon reports but does not control: whether the panel
//! enumerates, and what the vendor service is doing.
//!
//! Both are behind a trait so the state machine can be tested without libusb or
//! `sc.exe`.

/// One query of the vendor service. Identical in meaning to the UI's `ServiceStatus`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServiceSnapshot {
    pub supported: bool,
    pub running: bool,
    /// Combined stdout/stderr of `sc.exe query`, or the reason it does not apply.
    pub output: String,
}

pub trait HostEnvironment: Send {
    /// True when at least one matching device enumerates. This claims no interface, so
    /// it is safe while another process owns the panel.
    fn device_present(&mut self) -> bool;
    fn service(&mut self) -> ServiceSnapshot;
}

/// The real host: libusb enumeration plus `sc.exe` on Windows.
pub struct RealEnvironment;

impl HostEnvironment for RealEnvironment {
    fn device_present(&mut self) -> bool {
        mystique_usb::discover::present()
    }

    fn service(&mut self) -> ServiceSnapshot {
        crate::service::query()
    }
}
