//! Pure parsers for `/proc` and `/sys` text. No file I/O, so these run and are tested
//! on every platform.

/// Cumulative CPU jiffies from the first `cpu` line of `/proc/stat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuTimes {
    pub total: u64,
    pub idle: u64,
}

impl CpuTimes {
    /// `idle` plus `iowait`, the two fields that are not work.
    fn idle_all(self) -> u64 {
        self.idle
    }
}

/// Parses the aggregate `cpu` line.
///
/// Fields: user nice system idle iowait irq softirq steal guest guest_nice.
/// Only the aggregate is used; per-core parsing would add noise without display value.
pub fn parse_proc_stat(text: &str) -> Option<CpuTimes> {
    let line = text.lines().find(|line| line.starts_with("cpu "))?;
    let mut fields = line.split_whitespace();
    if fields.next()? != "cpu" {
        return None;
    }
    let values: Vec<u64> = fields.filter_map(|field| field.parse().ok()).collect();
    if values.len() < 4 {
        return None;
    }
    // idle and iowait are both "not doing work"; everything else is busy.
    let idle = values[3].saturating_add(values.get(4).copied().unwrap_or(0));
    let total = values.iter().copied().sum();
    Some(CpuTimes { total, idle })
}

/// CPU usage between two snapshots, as a percentage.
///
/// Returns `None` on the first sample (no previous snapshot) and whenever the counters
/// did not advance.
pub fn cpu_usage_percent(previous: CpuTimes, current: CpuTimes) -> Option<u8> {
    let total = current.total.checked_sub(previous.total)?;
    if total == 0 {
        return None;
    }
    let idle = current.idle_all().saturating_sub(previous.idle_all());
    let busy = total.saturating_sub(idle);
    Some(
        ((busy as f64 / total as f64) * 100.0)
            .round()
            .clamp(0.0, 100.0) as u8,
    )
}

/// Parses `MemTotal` and `MemAvailable` (KiB) from `/proc/meminfo`.
pub fn parse_meminfo(text: &str) -> Option<(u64, u64)> {
    let mut total = None;
    let mut available = None;
    for line in text.lines() {
        let (key, rest) = line.split_once(':')?;
        let value = rest.split_whitespace().next()?.parse::<u64>().ok()?;
        match key {
            "MemTotal" => total = Some(value),
            "MemAvailable" => available = Some(value),
            _ => {}
        }
        if total.is_some() && available.is_some() {
            break;
        }
    }
    Some((total?, available?))
}

/// Used-memory percentage from the two values above.
pub fn memory_load_percent(total_kib: u64, available_kib: u64) -> Option<u8> {
    if total_kib == 0 {
        return None;
    }
    let used = total_kib.saturating_sub(available_kib);
    Some(
        ((used as f64 / total_kib as f64) * 100.0)
            .round()
            .clamp(0.0, 100.0) as u8,
    )
}

/// `scaling_cur_freq` is kHz; the wire frame wants MHz.
pub fn khz_to_mhz(text: &str) -> Option<u16> {
    let khz: u64 = text.trim().parse().ok()?;
    u16::try_from(khz / 1000).ok().filter(|mhz| *mhz > 0)
}

/// Parses `cpu MHz : 4300.000` from `/proc/cpuinfo`, used when cpufreq is absent
/// (common in VMs).
pub fn parse_cpuinfo_mhz(text: &str) -> Option<u16> {
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("cpu MHz") {
            let mhz: f64 = value.trim().parse().ok()?;
            if mhz.is_finite() && mhz >= 0.0 {
                return u16::try_from(mhz.round() as u64).ok();
            }
        }
    }
    None
}

/// Reads a millidegree value (e.g. `temp1_input`) and converts to whole °C.
pub fn millidegrees_to_celsius(text: &str) -> Option<u8> {
    let milli: i64 = text.trim().parse().ok()?;
    let celsius = milli / 1000;
    if !(-40..=150).contains(&celsius) {
        return None;
    }
    u8::try_from(celsius).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT: &str = "\
cpu  100 0 100 800 0 0 0 0 0 0
cpu0 50 0 50 400 0 0 0 0 0 0
cpu1 50 0 50 400 0 0 0 0 0 0
intr 12345
";

    #[test]
    fn parses_the_aggregate_cpu_line() {
        let times = parse_proc_stat(STAT).unwrap();
        assert_eq!(times.total, 1000);
        assert_eq!(times.idle, 800);
    }

    #[test]
    fn computes_usage_between_snapshots() {
        let previous = CpuTimes {
            total: 1000,
            idle: 800,
        };
        let current = CpuTimes {
            total: 1200,
            idle: 850,
        };
        // 200 jiffies elapsed, 50 idle -> 150 busy -> 75%
        assert_eq!(cpu_usage_percent(previous, current), Some(75));
    }

    #[test]
    fn first_sample_and_stalled_counters_yield_none() {
        let times = CpuTimes {
            total: 1000,
            idle: 800,
        };
        assert_eq!(cpu_usage_percent(times, times), None);
        assert_eq!(
            cpu_usage_percent(
                times,
                CpuTimes {
                    total: 900,
                    idle: 800
                }
            ),
            None
        );
    }

    #[test]
    fn usage_is_clamped() {
        let previous = CpuTimes { total: 0, idle: 0 };
        let current = CpuTimes {
            total: 100,
            idle: 0,
        };
        assert_eq!(cpu_usage_percent(previous, current), Some(100));
    }

    #[test]
    fn parses_meminfo_in_any_order() {
        let text = "MemTotal:       16384000 kB\nMemFree:  100 kB\nMemAvailable:    8192000 kB\n";
        assert_eq!(parse_meminfo(text), Some((16_384_000, 8_192_000)));
        assert_eq!(memory_load_percent(16_384_000, 8_192_000), Some(50));
    }

    #[test]
    fn meminfo_without_available_is_none() {
        assert_eq!(parse_meminfo("MemTotal: 100 kB\nMemFree: 50 kB\n"), None);
        assert_eq!(memory_load_percent(0, 0), None);
    }

    #[test]
    fn converts_khz_to_mhz() {
        assert_eq!(khz_to_mhz("4300000\n"), Some(4300));
        assert_eq!(khz_to_mhz("0"), None);
        assert_eq!(khz_to_mhz("nonsense"), None);
    }

    #[test]
    fn parses_cpuinfo_mhz_and_ignores_other_lines() {
        let text = "processor\t: 0\nmodel name\t: Test\ncpu MHz\t\t: 3600.000\n";
        assert_eq!(parse_cpuinfo_mhz(text), Some(3600));
    }

    #[test]
    fn converts_millidegrees_and_rejects_implausible_values() {
        assert_eq!(millidegrees_to_celsius("44000"), Some(44));
        assert_eq!(millidegrees_to_celsius("-5000"), None);
        assert_eq!(millidegrees_to_celsius("250000"), None);
        assert_eq!(millidegrees_to_celsius("garbage"), None);
    }
}
