//! Windows telemetry: DeepCool's own sensor pipe first, `sysinfo` (WMI/PDH) second.
//!
//! The named pipe is **not required**. It exists only while DeepCool's helper service
//! is running, and the driver must work without it — the whole point of not depending
//! on the vendor app.

use sysinfo::{Components, System};

use crate::pipe::PipeSensor;
use crate::{Sampler, SensorSample};

pub struct WindowsSampler {
    system: System,
    components: Components,
    pipe: Option<PipeSensor>,
}

impl WindowsSampler {
    pub fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_cpu_usage();
        system.refresh_memory();
        Self {
            system,
            components: Components::new_with_refreshed_list(),
            pipe: PipeSensor::connect().ok(),
        }
    }

    /// True when the vendor sensor bridge was reachable at startup.
    pub fn has_vendor_pipe(&self) -> bool {
        self.pipe.is_some()
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
        if let Some(pipe) = self.pipe.as_mut() {
            match pipe.next_sample() {
                Ok(sample) => return sample,
                // A broken or closed pipe is a normal condition (vendor service
                // stopped). Drop it permanently and use sysinfo from here on.
                Err(_) => self.pipe = None,
            }
        }
        self.sample_sysinfo()
    }

    fn describe(&self) -> &'static str {
        if self.has_vendor_pipe() {
            "windows: deepcool_sensor_data pipe"
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

/// Windows-only helper used by the CLI to report whether the vendor pipe was seen.
pub fn vendor_pipe_present() -> bool {
    PipeSensor::connect().is_ok()
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
            pipe: None,
        };
        let sample = sampler.sample();
        let _ = sample.to_frame();
        assert!(!sampler.describe().is_empty());
    }
}
