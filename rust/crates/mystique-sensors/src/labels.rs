//! CPU and GPU names for the panel's two text labels.
//!
//! Labels used to be a command-line argument, which meant the owner had to type
//! `"Intel Core i9-14900K"` exactly. This module answers the same question from the
//! host: the first CPU's brand from `sysinfo`, and the display adapters the OS reports.
//!
//! Detection never fails the caller. A missing `powershell`, a machine with no GPU or
//! an unparseable `lspci` line yields an empty list, and the label stays free text.
//! Nothing here touches the panel; the daemon decides whether to offer the names.

#[cfg(any(windows, target_os = "linux"))]
use std::process::Command;

use sysinfo::{CpuRefreshKind, RefreshKind, System};

/// What this host offers as label values. Order is the detection order; duplicates are
/// removed. An empty list means "detection found nothing", not "no labels allowed".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DetectedLabels {
    pub cpu: Vec<String>,
    pub gpu: Vec<String>,
}

/// Detects the labels. Blocking on Windows (`powershell`) and Linux (`lspci`), so a
/// caller keeps it off a redraw path.
pub fn detect() -> DetectedLabels {
    DetectedLabels {
        cpu: cpu_names(),
        gpu: gpu_names(),
    }
}

/// The first CPU's `sysinfo` brand, or nothing when sysinfo cannot read it.
fn cpu_names() -> Vec<String> {
    let system =
        System::new_with_specifics(RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()));
    let brand = system
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim())
        .unwrap_or_default();
    if brand.is_empty() {
        Vec::new()
    } else {
        vec![brand.to_owned()]
    }
}

/// Non-empty, trimmed lines with duplicates removed; the first occurrence wins.
///
/// Only the Windows probe yields whole lines. The Linux probe parses `lspci -mm` into
/// fields and deduplicates afterwards, so this helper is not built there.
#[cfg(windows)]
fn clean_lines(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for line in text.lines() {
        let name = line.trim();
        if !name.is_empty() && !names.iter().any(|seen| seen == name) {
            names.push(name.to_owned());
        }
    }
    names
}

/// `Get-CimInstance Win32_VideoController` names. The same adapter can appear twice
/// (one entry per driver instance), which is why the lines are deduplicated.
#[cfg(windows)]
fn gpu_names() -> Vec<String> {
    use std::os::windows::process::CommandExt;

    /// Do not flash a console window when the daemon has none of its own.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-CimInstance Win32_VideoController).Name",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match output {
        Ok(output) if output.status.success() => {
            clean_lines(&String::from_utf8_lossy(&output.stdout))
        }
        _ => Vec::new(),
    }
}

/// Device names (`lspci -mm`, fourth field) of every VGA, 3D or display controller.
///
/// `lspci -mm` prints `slot "class" "vendor" "device"`; only the class chooses the entry
/// and only the device names the adapter, so the vendor string is not repeated.
#[cfg(target_os = "linux")]
fn gpu_names() -> Vec<String> {
    let output = Command::new("lspci").arg("-mm").output();
    match output {
        Ok(output) if output.status.success() => {
            parse_lspci(&String::from_utf8_lossy(&output.stdout))
        }
        _ => Vec::new(),
    }
}

#[cfg(target_os = "linux")]
fn parse_lspci(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for line in text.lines() {
        let fields = quoted_fields(line);
        let Some(class) = fields.first() else {
            continue;
        };
        let class = class.to_ascii_lowercase();
        if !(class.contains("vga") || class.contains("3d") || class.contains("display")) {
            continue;
        }
        let Some(device) = fields.get(2) else {
            continue;
        };
        let name = device.trim();
        if !name.is_empty() && !names.iter().any(|seen| seen == name) {
            names.push(name.to_owned());
        }
    }
    names
}

/// The quoted fields of one `lspci -mm` line. `lspci` escapes `"` and `\` inside them,
/// so a plain `split('"')` would cut a name in half.
#[cfg(target_os = "linux")]
fn quoted_fields(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for ch in line.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
        } else if quoted && ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            if quoted {
                fields.push(std::mem::take(&mut current));
            }
            quoted = !quoted;
        } else if quoted {
            current.push(ch);
        }
    }
    fields
}

/// Other platforms have no probe this crate trusts, so they offer nothing.
#[cfg(not(any(windows, target_os = "linux")))]
fn gpu_names() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn lines_are_trimmed_and_duplicates_keep_their_first_position() {
        let text = "NVIDIA GeForce RTX 4070\r\n\r\n Intel(R) UHD Graphics 770 \n\
                    NVIDIA GeForce RTX 4070\n";
        assert_eq!(
            clean_lines(text),
            vec![
                "NVIDIA GeForce RTX 4070".to_owned(),
                "Intel(R) UHD Graphics 770".to_owned(),
            ]
        );
        assert!(clean_lines("  \n\t\n").is_empty());
    }

    #[test]
    fn the_cpu_brand_is_one_name_or_nothing() {
        let labels = detect();
        assert!(labels.cpu.len() <= 1, "{:?}", labels.cpu);
        assert!(labels.cpu.iter().all(|name| !name.trim().is_empty()));
        // A GPU list is allowed to be empty: headless hosts exist.
        assert!(labels.gpu.iter().all(|name| !name.trim().is_empty()));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn lspci_keeps_display_classes_and_drops_the_rest() {
        let text = concat!(
            "00:02.0 \"VGA compatible controller\" \"Intel Corporation\" \"Raptor Lake-S GT1 [UHD Graphics 770]\" -r04 \"Dell\" \"Device 0a9d\"\n",
            "01:00.0 \"3D controller\" \"NVIDIA Corporation\" \"AD104 [GeForce RTX 4070]\" -ra01 \"ASUSTeK\" \"Device 88c8\"\n",
            "00:14.0 \"USB controller\" \"Intel Corporation\" \"Raptor Lake USB 3.2 Gen 2x2 XHCI Controller\" -r11 \"Dell\" \"Device 0a9d\"\n",
            "00:02.0 \"VGA compatible controller\" \"Intel Corporation\" \"Raptor Lake-S GT1 [UHD Graphics 770]\" -r04 \"Dell\" \"Device 0a9d\"\n",
            "02:00.0 \"Display controller\" \"Intel Corporation\" \"Device 1234\"\n",
        );
        assert_eq!(
            parse_lspci(text),
            vec![
                "Raptor Lake-S GT1 [UHD Graphics 770]".to_owned(),
                "AD104 [GeForce RTX 4070]".to_owned(),
                "Device 1234".to_owned(),
            ]
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn escaped_quotes_in_an_lspci_line_survive() {
        let fields = quoted_fields("00:00.0 \"Class \\\"quoted\\\"\" \"Vendor\" \"Device\"");
        assert_eq!(fields, ["Class \"quoted\"", "Vendor", "Device"]);
    }
}
