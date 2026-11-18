//! Windows telemetry: DeepCool's own sensor pipe first, `sysinfo` (WMI/PDH) second.
//!
//! The named pipe is **not required**. It exists only while DeepCool's helper service
//! is running, and the driver must work without it — the whole point of not depending
//! on the vendor app.
//!
//! The pipe pushes once per ~1.1 s, so it is read on a thread of its own
//! ([`PipeFeed`]) and this sampler only ever copies the newest snapshot. A blocking
//! read on the sampling path would cap the driver at the vendor's push rate, which it
//! did until it was measured.

use std::time::Duration;

use sysinfo::{Components, System};

use crate::pipe::PipeFeed;
use crate::{Sampler, SensorSample};

/// How old a vendor snapshot may be before the sampler prefers a live sysinfo read.
/// The bridge pushes every ~1.1 s, so this tolerates one missed push.
const VENDOR_MAX_AGE: Duration = Duration::from_secs(3);

pub struct WindowsSampler {
    system: System,
    components: Components,
    vendor: Option<PipeFeed>,
    last_used_vendor: bool,
}

impl WindowsSampler {
    pub fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_cpu_usage();
        system.refresh_memory();
        Self {
            system,
            components: Components::new_with_refreshed_list(),
            vendor: PipeFeed::start(),
            last_used_vendor: false,
        }
    }

    /// True when the vendor sensor bridge was reachable at startup.
    pub fn has_vendor_pipe(&self) -> bool {
        self.vendor.is_some()
    }

    fn sample_sysinfo(&mut self) -> SensorSample {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.components.refresh(true);

        let cpu_usage = percentage(self.system.global_cpu_usage());
        let total = self.system.total_memory();
        let used = self.system.used_memory();
        let memory_load = if total == 0 {
            None
        } else {
            percentage((used as f32 / total as f32) * 100.0)
        };

        let cpu_clock_mhz = self
            .system
            .cpus()
            .first()
            .map(|cpu| cpu.frequency())
            .and_then(|mhz| u16::try_from(mhz).ok())
            .filter(|mhz| *mhz > 0);

        SensorSample {
            cpu_temperature_c: component_temperature(&self.components, &["cpu", "core", "package"]),
            cpu_usage_percent: cpu_usage,
            memory_load_percent: memory_load,
            gpu_temperature_c: component_temperature(
                &self.components,
                &["gpu", "nvidia", "radeon"],
            ),
            cpu_clock_mhz,
            // `sysinfo` exposes no fan sensors. The vendor bridge is the only fan source
            // on Windows, so without it the frame carries 0, which the panel draws as
            // 0 RPM — the same thing the vendor app shows with no mapping configured.
            pump_rpm: None,
            cpu_fan_rpm: None,
        }
    }
}

impl Default for WindowsSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Sampler for WindowsSampler {
    fn sample(&mut self) -> SensorSample {
        // A copy of the newest vendor snapshot, when the bridge is alive and not
        // stalled. This costs microseconds, so the stream rate is decided by the link,
        // not by the sensor source.
        if let Some(sample) = self
            .vendor
            .as_ref()
            .and_then(|feed| feed.fresh(VENDOR_MAX_AGE))
        {
            self.last_used_vendor = true;
            return sample;
        }
        self.last_used_vendor = false;
        self.sample_sysinfo()
    }

    fn describe(&self) -> &'static str {
        if self.last_used_vendor {
            "windows: deepcool_sensor_data pipe (1.1 s push)"
        } else if self.vendor.is_some() {
            "windows: sysinfo WMI/PDH (vendor pipe stalled)"
        } else {
            "windows: sysinfo WMI/PDH (vendor pipe absent)"
        }
    }
}

/// Converts a value to a frame byte, replacing nonsense with `None` rather than a
/// saturating lie.
fn percentage(value: f32) -> Option<u8> {
    if value.is_finite() && (0.0..=100.0).contains(&value) {
        Some(value.round() as u8)
    } else {
        None
    }
}

fn component_temperature(components: &Components, needles: &[&str]) -> Option<u8> {
    components
        .iter()
        .filter_map(|component| {
            let label = component.label().to_ascii_lowercase();
            let matches = needles.iter().any(|needle| label.contains(needle));
            matches
                .then(|| component.temperature())
                .flatten()
                .filter(|celsius| celsius.is_finite() && (-40.0..=150.0).contains(celsius))
        })
        .next()
        .map(|celsius| celsius.round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentage_rejects_nonsense() {
        assert_eq!(percentage(42.4), Some(42));
        assert_eq!(percentage(0.0), Some(0));
        assert_eq!(percentage(100.0), Some(100));
        assert_eq!(percentage(100.5), None);
        assert_eq!(percentage(f32::NAN), None);
        assert_eq!(percentage(-1.0), None);
    }

    #[test]
    fn a_sampler_can_be_created_without_the_vendor_pipe() {
        // Never touch the live vendor pipe in a test: on a machine where the service
        // is running, a connect would succeed and the first sample would block.
        let mut sampler = WindowsSampler {
            system: System::new(),
            components: Components::new_with_refreshed_list(),
            vendor: None,
            last_used_vendor: false,
        };
        let sample = sampler.sample();
        let _ = sample.to_frame();
        assert!(!sampler.describe().is_empty());
    }
}
