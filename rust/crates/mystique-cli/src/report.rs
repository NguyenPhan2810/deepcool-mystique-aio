//! The exploration report — the deliverable.
//!
//! Every field is written on every run, in declaration order, so two reports diff
//! cleanly. `Option` fields become `null` rather than being omitted: a missing key and
//! a key with no value are different things, and the latter is what actually happens.

use std::path::Path;

use anyhow::{Context, Result};
use mystique_protocol::commands::SAFE_ALLOWLIST;
use mystique_usb::DeviceSummary;
use serde::Serialize;

pub const REPORT_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
pub struct Report {
    pub version: u32,
    pub tool: String,
    pub generated_at: String,
    /// `presence` | `delta` | `payload` | `sequence`
    pub phase: String,
    /// `send` | `unsafe-all`
    pub mode: String,
    pub scope: Scope,
    /// Commands the conservative gate allows without `--unsafe-all`.
    pub allowlist: Vec<u8>,
    /// Commands the operator excluded with `--skip`.
    pub skipped: Vec<u8>,
    pub probe_timeout_ms: u64,
    pub device: DeviceReport,
    pub redaction: Redaction,
    /// Present only when the sweep stopped on a blocked write.
    pub wedge: Option<Wedge>,
    pub summary: Summary,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Serialize)]
pub struct Scope {
    pub first: u8,
    pub last: u8,
    pub count: usize,
}

impl Scope {
    pub fn full() -> Self {
        Self {
            first: 0x00,
            last: 0xFF,
            count: 256,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct DeviceReport {
    pub vendor_id: u16,
    pub product_id: u16,
    pub bus: u8,
    pub address: u8,
    /// `bcdDevice`; `0x0246` on this unit.
    pub release: u16,
    pub product: Option<String>,
    /// Always true. The serial and the 0x12 identifier are never written.
    pub identifiers_redacted: bool,
}

impl From<&DeviceSummary> for DeviceReport {
    fn from(summary: &DeviceSummary) -> Self {
        Self {
            vendor_id: summary.vendor_id,
            product_id: summary.product_id,
            bus: summary.bus,
            address: summary.address,
            release: summary.release,
            product: summary.product.clone(),
            identifiers_redacted: true,
        }
    }
}

impl DeviceReport {
    /// Descriptor summary for a live device. Falls back to the known IDs when the
    /// descriptors cannot be re-read; probing should not depend on a cosmetic read.
    pub fn from_device(device: &mystique_usb::MystiqueDevice) -> Self {
        match device.summary() {
            Ok(summary) => Self::from(&summary),
            Err(_) => Self {
                vendor_id: mystique_usb::discover::VENDOR_ID,
                product_id: mystique_usb::discover::PRODUCT_ID,
                bus: 0,
                address: 0,
                release: 0,
                product: None,
                identifiers_redacted: true,
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Redaction {
    pub policy: &'static str,
}

impl Default for Redaction {
    fn default() -> Self {
        Self {
            policy: "bytes 4..36 masked when they match the 32-uppercase-hex identifier shape; serial and device id never written",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Wedge {
    pub detected: bool,
    pub phase: String,
    pub at_cmd: u8,
    pub detail: String,
    pub recovery: String,
}

/// One probe. The seven required keys come first; the phase-specific extras follow.
#[derive(Debug, Serialize)]
pub struct Entry {
    pub index: usize,
    pub cmd: u8,
    pub pipe: String,
    /// Empty string for an empty payload.
    pub payload_hex: String,
    /// Phase-specific label, e.g. `init 0x02` or `payload: ones`. `null` when unused.
    pub label: Option<String>,
    pub replied: bool,
    pub echo: Option<u8>,
    /// Raw response byte 3. `null` when no reply arrived.
    pub result_code: Option<u8>,
    pub reply_len: Option<usize>,
    pub reply_hex_redacted: Option<String>,
    pub elapsed_ms: u64,
    /// `reply` | `timeout` | `stall` | `disconnected` | `error`
    pub outcome: String,
    pub error: Option<String>,
    pub status_before: Option<StatusSnapshot>,
    pub status_after: Option<StatusSnapshot>,
}

impl Entry {
    pub fn new(index: usize, cmd: u8, pipe: &str, payload: &[u8]) -> Self {
        Self {
            index,
            cmd,
            pipe: pipe.to_owned(),
            payload_hex: hex(payload),
            label: None,
            replied: false,
            echo: None,
            result_code: None,
            reply_len: None,
            reply_hex_redacted: None,
            elapsed_ms: 0,
            outcome: "error".to_owned(),
            error: None,
            status_before: None,
            status_after: None,
        }
    }

    pub const fn succeeded(&self) -> bool {
        matches!(self.result_code, Some(0x00))
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct StatusSnapshot {
    pub rotation: u8,
    pub degrees: Option<u16>,
    pub mode_raw: u8,
    pub mode: &'static str,
    pub result_code: u8,
}

impl From<mystique_protocol::PanelStatus> for StatusSnapshot {
    fn from(status: mystique_protocol::PanelStatus) -> Self {
        Self {
            rotation: status.rotation,
            degrees: status.rotation_degrees(),
            mode_raw: status.raw_mode,
            mode: status.mode.label(),
            result_code: status.result.as_raw(),
        }
    }
}

#[derive(Debug, Default, Serialize)]
pub struct Summary {
    /// Probes actually sent.
    pub probed: usize,
    pub replied: usize,
    pub success: usize,
    pub timeouts: usize,
    pub errors: usize,
    /// Delta phase: commands whose status snapshot changed.
    pub state_changes: usize,
    /// Delta/payload phases: commands that replied successfully in the internal
    /// presence sweep. Zero in a presence report.
    pub discovered: usize,
}

impl Summary {
    pub fn from_entries(entries: &[Entry]) -> Self {
        let mut summary = Self {
            probed: entries.len(),
            ..Self::default()
        };
        for entry in entries {
            if entry.replied {
                summary.replied += 1;
            }
            if entry.succeeded() {
                summary.success += 1;
            }
            match entry.outcome.as_str() {
                "timeout" => summary.timeouts += 1,
                "error" | "stall" | "disconnected" => summary.errors += 1,
                _ => {}
            }
        }
        summary
    }

    pub fn count_state_changes(&mut self, entries: &[Entry]) {
        self.state_changes = entries
            .iter()
            .filter(|entry| {
                matches!((entry.status_before, entry.status_after), (Some(before), Some(after))
                    if before.rotation != after.rotation
                        || before.mode_raw != after.mode_raw
                        || before.result_code != after.result_code)
            })
            .count();
    }
}

impl Report {
    pub fn new_header(phase: &str, mode: &str, timeout_ms: u64, device: DeviceReport) -> Self {
        Self {
            version: REPORT_VERSION,
            tool: format!("mystique {} explore", env!("CARGO_PKG_VERSION")),
            generated_at: crate::timefmt::rfc3339(),
            phase: phase.to_owned(),
            mode: mode.to_owned(),
            scope: Scope::full(),
            allowlist: SAFE_ALLOWLIST.to_vec(),
            skipped: Vec::new(),
            probe_timeout_ms: timeout_ms,
            device,
            redaction: Redaction::default(),
            wedge: None,
            summary: Summary::default(),
            entries: Vec::new(),
        }
    }

    /// Writes pretty JSON plus a trailing newline, creating parent directories.
    pub fn write(&mut self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self).context("serialising the report")?;
        std::fs::write(path, format!("{json}\n"))
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_required_keys_are_always_serialised() {
        let entry = Entry::new(0, 0x10, "data", &[]);
        let json = serde_json::to_value(&entry).unwrap();
        for key in [
            "cmd",
            "replied",
            "echo",
            "result_code",
            "reply_len",
            "reply_hex_redacted",
            "elapsed_ms",
        ] {
            assert!(json.get(key).is_some(), "missing {key}");
        }
        assert!(json["echo"].is_null());
        assert_eq!(json["cmd"], 0x10);
    }

    #[test]
    fn key_order_is_stable_and_declaration_ordered() {
        let entry = Entry::new(0, 0x10, "data", &[]);
        let text = serde_json::to_string(&entry).unwrap();
        let cmd = text.find("\"cmd\"").unwrap();
        let replied = text.find("\"replied\"").unwrap();
        let elapsed = text.find("\"elapsed_ms\"").unwrap();
        assert!(cmd < replied && replied < elapsed);
    }

    #[test]
    fn summary_counts_outcomes() {
        let mut entries = Vec::new();
        let mut a = Entry::new(0, 0x10, "data", &[]);
        a.replied = true;
        a.result_code = Some(0);
        a.outcome = "reply".into();
        entries.push(a);

        let mut b = Entry::new(1, 0x11, "data", &[]);
        b.outcome = "timeout".into();
        entries.push(b);

        let summary = Summary::from_entries(&entries);
        assert_eq!(summary.probed, 2);
        assert_eq!(summary.replied, 1);
        assert_eq!(summary.success, 1);
        assert_eq!(summary.timeouts, 1);
    }

    #[test]
    fn a_zero_length_payload_still_serialises_as_an_empty_string() {
        let entry = Entry::new(0, 0x10, "data", &[]);
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["payload_hex"], "");
    }
}
