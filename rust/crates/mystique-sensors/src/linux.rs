//! Linux telemetry: `/proc` and `/sys`, with `nvidia-smi` as the only subprocess.
//!
//! Nothing here is privileged. `hwmon` temperatures are world-readable; DRM hwmon
//! nodes are too. `nvidia-smi` is only invoked when no DRM hwmon reports a GPU, and a
//! missing binary is simply `None`.

use std::fs;
use std::path::Path;
use std::process::Command;

use crate::procfs::{self, CpuTimes};
use crate::{Sampler, SensorSample};

/// hwmon drivers that report a CPU package temperature.
const CPU_HWMON: &[&str] = &["coretemp", "k10temp", "zenpower", "cpu_thermal"];
/// hwmon drivers that report a GPU temperature.
const GPU_HWMON: &[&str] = &["amdgpu", "radeon", "nouveau", "i915", "xe"];

pub struct LinuxSampler {
    previous_cpu: Option<CpuTimes>,
}

impl LinuxSampler {
    pub fn new() -> Self {
        // Prime the delta so the first sample already has a usage figure. Without
        // this the first frame would show 0%.
        let previous_cpu = read_text("/proc/stat").and_then(|text| procfs::parse_proc_stat(&text));
        Self { previous_cpu }
    }

    fn cpu_usage(&mut self) -> Option<u8> {
        let current = read_text("/proc/stat").and_then(|text| procfs::parse_proc_stat(&text))?;
        let usage = self
            .previous_cpu
            .and_then(|previous| procfs::cpu_usage_percent(previous, current));
        self.previous_cpu = Some(current);
        usage
    }
}

impl Default for LinuxSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Sampler for LinuxSampler {
    fn sample(&mut self) -> SensorSample {
        SensorSample {
            cpu_temperature_c: cpu_temperature(),
            cpu_usage_percent: self.cpu_usage(),
            memory_load_percent: read_text("/proc/meminfo")
                .and_then(|text| procfs::parse_meminfo(&text))
                .and_then(|(total, available)| procfs::memory_load_percent(total, available)),
            gpu_temperature_c: gpu_temperature(),
            cpu_clock_mhz: cpu_clock_mhz(),
        }
    }

    fn describe(&self) -> &'static str {
        "linux: /proc/stat, /proc/meminfo, /sys/class/hwmon, nvidia-smi fallback"
    }
}

fn read_text(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn cpu_temperature() -> Option<u8> {
    for entry in fs::read_dir("/sys/class/hwmon").ok()?.flatten() {
        let dir = entry.path();
        let Some(name) = read_text(dir.join("name")) else {
            continue;
        };
        if !CPU_HWMON.contains(&name.trim()) {
            continue;
        }
        if let Some(celsius) = read_text(dir.join("temp1_input"))
            .and_then(|text| procfs::millidegrees_to_celsius(&text))
        {
            return Some(celsius);
        }
    }

    // Newer kernels expose the package temperature as a thermal zone rather than a
    // hwmon device.
    for entry in fs::read_dir("/sys/class/thermal").ok()?.flatten() {
        let dir = entry.path();
        let Some(kind) = read_text(dir.join("type")) else {
            continue;
        };
        if kind.trim() != "x86_pkg_temp" {
            continue;
        }
        if let Some(celsius) =
            read_text(dir.join("temp")).and_then(|text| procfs::millidegrees_to_celsius(&text))
        {
            return Some(celsius);
        }
    }

    None
}

fn gpu_temperature() -> Option<u8> {
    if let Ok(entries) = fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_card = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("card") && !name.contains('-'));
            if !is_card {
                continue;
            }

            let Ok(hwmons) = fs::read_dir(path.join("device/hwmon")) else {
                continue;
            };
            for hwmon in hwmons.flatten() {
                let dir = hwmon.path();
                let name = read_text(dir.join("name")).unwrap_or_default();
                if !GPU_HWMON.contains(&name.trim()) {
                    continue;
                }
                if let Some(celsius) = read_text(dir.join("temp1_input"))
                    .and_then(|text| procfs::millidegrees_to_celsius(&text))
                {
                    return Some(celsius);
                }
            }
        }
    }

    nvidia_smi_temperature()
}

fn nvidia_smi_temperature() -> Option<u8> {
    // Blocking, but nvidia-smi answers in tens of milliseconds and this is only
    // reached when no in-kernel driver exposes a temperature.
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=temperature.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .next()?
        .trim()
        .parse::<u8>()
        .ok()
        .filter(|celsius| *celsius <= 150)
}

fn cpu_clock_mhz() -> Option<u16> {
    read_text("/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq")
        .and_then(|text| procfs::khz_to_mhz(&text))
        .or_else(|| read_text("/proc/cpuinfo").and_then(|text| procfs::parse_cpuinfo_mhz(&text)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sampler_is_constructible_without_any_sensor() {
        // On a machine without /sys or /proc this must still construct and sample.
        let mut sampler = LinuxSampler::new();
        let _ = sampler.sample();
        assert!(!sampler.describe().is_empty());
    }
}
