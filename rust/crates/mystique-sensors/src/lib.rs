//! Cross-platform telemetry sources.
//!
//! Every field is optional. A missing sensor is normal — desktops have no battery,
//! VMs have no `coretemp`, containers have no `/sys` — and must never panic or abort
//! the stream. [`SensorSample::to_frame`] substitutes the frame's safe default (zero)
//! for anything absent, which is what the device will display.

#![forbid(unsafe_code)]

pub mod pipe;
pub mod procfs;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(windows)]
pub mod windows;

use mystique_protocol::TelemetryFrame;

/// One reading from the host. All fields are optional by design.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SensorSample {
    pub cpu_temperature_c: Option<u8>,
    pub cpu_usage_percent: Option<u8>,
    pub memory_load_percent: Option<u8>,
    pub gpu_temperature_c: Option<u8>,
    pub cpu_clock_mhz: Option<u16>,
    /// `Fan list -> "AIO Pump"` on Windows; a hwmon channel labelled "pump" on Linux.
    pub pump_rpm: Option<u16>,
    /// `Fan list -> "CPU"` on Windows; the first unlabelled fan channel on Linux.
    pub cpu_fan_rpm: Option<u16>,
}

impl SensorSample {
    /// Converts to the wire frame, substituting zero for absent sensors.
    ///
    /// The substitution is deliberate and documented rather than hidden: the protocol
    /// has no "unknown" encoding, so a driver must send something.
    pub fn to_frame(self) -> TelemetryFrame {
        TelemetryFrame {
            cpu_temperature_c: self.cpu_temperature_c.unwrap_or(0),
            cpu_usage_percent: self.cpu_usage_percent.unwrap_or(0),
            memory_load_percent: self.memory_load_percent.unwrap_or(0),
            gpu_temperature_c: self.gpu_temperature_c.unwrap_or(0),
            cpu_clock_mhz: self.cpu_clock_mhz.unwrap_or(0),
            pump_rpm: self.pump_rpm.unwrap_or(0),
            cpu_fan_rpm: self.cpu_fan_rpm.unwrap_or(0),
        }
    }

    pub fn missing_fields(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if self.cpu_temperature_c.is_none() {
            missing.push("cpu_temperature_c");
        }
        if self.cpu_usage_percent.is_none() {
            missing.push("cpu_usage_percent");
        }
        if self.memory_load_percent.is_none() {
            missing.push("memory_load_percent");
        }
        if self.gpu_temperature_c.is_none() {
            missing.push("gpu_temperature_c");
        }
        if self.cpu_clock_mhz.is_none() {
            missing.push("cpu_clock_mhz");
        }
        if self.pump_rpm.is_none() {
            missing.push("pump_rpm");
        }
        if self.cpu_fan_rpm.is_none() {
            missing.push("cpu_fan_rpm");
        }
        missing
    }
}

/// A source of samples. Implementations must be infallible: a sensor that cannot be
/// read yields `None` for that field, not an error.
///
/// `Send` is part of the contract because a daemon samples from a worker thread.
pub trait Sampler: Send {
    fn sample(&mut self) -> SensorSample;

    /// Human-readable source description, for the CLI header.
    fn describe(&self) -> &'static str;
}

/// Returns a sampler for the host platform, or one that yields all `None`.
pub fn default_sampler() -> Box<dyn Sampler> {
    #[cfg(windows)]
    {
        Box::new(windows::WindowsSampler::new())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxSampler::new())
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Box::new(EmptySampler)
    }
}

/// A sampler with no source; every field is `None`.
pub struct EmptySampler;

impl Sampler for EmptySampler {
    fn sample(&mut self) -> SensorSample {
        SensorSample::default()
    }

    fn describe(&self) -> &'static str {
        "none (unsupported platform)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_fields_become_zero_not_a_panic() {
        let frame = SensorSample::default().to_frame();
        assert_eq!(frame, TelemetryFrame::default());
        assert_eq!(SensorSample::default().missing_fields().len(), 7);
    }

    #[test]
    fn present_fields_survive() {
        let sample = SensorSample {
            cpu_temperature_c: Some(44),
            cpu_usage_percent: Some(7),
            memory_load_percent: Some(51),
            gpu_temperature_c: Some(60),
            cpu_clock_mhz: Some(4900),
            pump_rpm: Some(3341),
            cpu_fan_rpm: Some(1326),
        };
        assert!(sample.missing_fields().is_empty());
        let frame = sample.to_frame();
        assert_eq!(frame.cpu_temperature_c, 44);
        assert_eq!(frame.cpu_clock_mhz, 4900);
        assert_eq!(frame.pump_rpm, 3341);
        assert_eq!(frame.cpu_fan_rpm, 1326);
    }

    #[test]
    fn the_empty_sampler_is_safe() {
        let mut sampler = EmptySampler;
        assert_eq!(sampler.sample(), SensorSample::default());
        assert!(!sampler.describe().is_empty());
    }
}
