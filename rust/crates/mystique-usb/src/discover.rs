//! Device discovery by VID/PID and descriptor summary.

use rusb::{Device, GlobalContext};

use crate::error::DeviceError;

/// Vendor id, verified from PnP enumeration (`re/findings/00-hardware-identity.md`).
pub const VENDOR_ID: u16 = 0x3633;
/// Product id, verified.
pub const PRODUCT_ID: u16 = 0x0009;
/// Interface 0. The Windows code queries it and finds exactly four endpoints.
pub const INTERFACE: u8 = 0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointSummary {
    pub address: u8,
    pub direction: &'static str,
    pub transfer: &'static str,
    pub max_packet_size: u16,
    pub interval: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceSummary {
    pub bus: u8,
    pub address: u8,
    pub vendor_id: u16,
    pub product_id: u16,
    /// `bcdDevice`. This unit reports `0x0246`.
    pub release: u16,
    pub product: Option<String>,
    pub manufacturer: Option<String>,
    pub configurations: u8,
    pub interfaces: usize,
    pub endpoints: Vec<EndpointSummary>,
}

/// First device matching the MYSTIQUE VID/PID.
pub fn find() -> Result<Device<GlobalContext>, DeviceError> {
    let devices = rusb::devices().map_err(DeviceError::from)?;
    for device in devices.iter() {
        let descriptor = match device.device_descriptor() {
            Ok(descriptor) => descriptor,
            // A device can disappear between enumeration and descriptor read.
            Err(_) => continue,
        };
        if descriptor.vendor_id() == VENDOR_ID && descriptor.product_id() == PRODUCT_ID {
            return Ok(device);
        }
    }
    Err(DeviceError::NotFound)
}

/// True when at least one matching device enumerates.
pub fn present() -> bool {
    find().is_ok()
}

/// Reads descriptors only; no interface is claimed, so this is safe to call while
/// another process owns the device (enumeration still succeeds).
pub fn summarize(device: &Device<GlobalContext>) -> Result<DeviceSummary, DeviceError> {
    let descriptor = device.device_descriptor().map_err(DeviceError::from)?;

    // String descriptors need a handle. Failure to open (e.g. access denied) must not
    // hide the descriptors we can already read.
    let (product, manufacturer) = match device.open() {
        Ok(handle) => (
            handle.read_product_string_ascii(&descriptor).ok(),
            handle.read_manufacturer_string_ascii(&descriptor).ok(),
        ),
        Err(_) => (None, None),
    };

    let mut interfaces = 0;
    let mut endpoints = Vec::new();
    if let Ok(config) = device.active_config_descriptor() {
        for interface in config.interfaces() {
            for setting in interface.descriptors() {
                interfaces += 1;
                for endpoint in setting.endpoint_descriptors() {
                    endpoints.push(EndpointSummary {
                        address: endpoint.address(),
                        direction: if endpoint.address() & 0x80 != 0 {
                            "in"
                        } else {
                            "out"
                        },
                        transfer: transfer_label(endpoint.transfer_type()),
                        max_packet_size: endpoint.max_packet_size(),
                        interval: endpoint.interval(),
                    });
                }
            }
        }
    }

    Ok(DeviceSummary {
        bus: device.bus_number(),
        address: device.address(),
        vendor_id: descriptor.vendor_id(),
        product_id: descriptor.product_id(),
        release: (u16::from(descriptor.device_version().major()) << 8)
            | (u16::from(descriptor.device_version().minor()) << 4)
            | u16::from(descriptor.device_version().sub_minor()),
        product,
        manufacturer,
        configurations: descriptor.num_configurations(),
        interfaces,
        endpoints,
    })
}

fn transfer_label(transfer: rusb::TransferType) -> &'static str {
    match transfer {
        rusb::TransferType::Control => "control",
        rusb::TransferType::Isochronous => "isochronous",
        rusb::TransferType::Bulk => "bulk",
        rusb::TransferType::Interrupt => "interrupt",
    }
}
