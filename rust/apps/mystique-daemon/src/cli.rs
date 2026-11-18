//! Command line. The daemon is deliberately small: an interval, the init rotation and
//! labels, an endpoint override, and a `--service` flag that fails clearly until a real
//! Windows service wrapper exists.

use clap::Parser;
use mystique_protocol::Rotation;

use crate::state::validate_label;

#[derive(Debug, Parser)]
#[command(
    name = "mystique-daemon",
    version,
    about = "Headless owner of the DeepCool MYSTIQUE AIO LCD: telemetry and a control socket.",
    long_about = None
)]
pub struct Options {
    /// Telemetry period in milliseconds (8..=60000). 33 ms is 30 Hz.
    #[arg(long, default_value_t = 1000, value_parser = parse_interval)]
    pub interval_ms: u64,

    /// Rotation applied by the machine-info init (0, 1, 2, 3 = 0/90/180/270°).
    #[arg(long, default_value_t = 0, value_parser = parse_rotation)]
    pub rotation: u8,

    /// CPU label sent with 0x15 and included in the init handshake.
    #[arg(long, default_value = "--")]
    pub cpu: String,

    /// GPU label sent with 0x16 and included in the init handshake.
    #[arg(long, default_value = "--")]
    pub gpu: String,

    /// Override the control endpoint. Windows: a pipe name or `\\.\pipe\...`.
    /// Unix: an absolute socket path.
    #[arg(long)]
    pub endpoint: Option<String>,

    /// Windows service mode. Not implemented in this build; fails clearly.
    #[arg(long)]
    pub service: bool,
}

impl Options {
    /// Validates everything clap cannot: label contents and the rotation range.
    pub fn validate(&self) -> Result<Rotation, String> {
        validate_label(&self.cpu, "CPU")?;
        validate_label(&self.gpu, "GPU")?;
        Rotation::new(self.rotation).map_err(|error| error.to_string())
    }
}

/// Smallest accepted period. The link is a full-speed bulk pipe and a frame is 48 bytes;
/// 8 ms (125 Hz) is far inside the bus budget, so this is a guard against a typo, not a
/// performance limit. The panel's real ceiling is measured, not assumed.
pub const MIN_INTERVAL_MS: u64 = 8;

fn parse_interval(text: &str) -> Result<u64, String> {
    let value: u64 = text
        .parse()
        .map_err(|_| format!("\"{text}\" is not a whole number of milliseconds"))?;
    if !(MIN_INTERVAL_MS..=60_000).contains(&value) {
        return Err(format!(
            "interval {value} ms is outside the supported range {MIN_INTERVAL_MS}..=60000"
        ));
    }
    Ok(value)
}

fn parse_rotation(text: &str) -> Result<u8, String> {
    let value: u8 = text
        .parse()
        .map_err(|_| format!("\"{text}\" is not a rotation index"))?;
    if value > 3 {
        return Err(format!(
            "rotation {value} is out of range; expected 0, 1, 2 or 3"
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> Options {
        Options {
            interval_ms: 1000,
            rotation: 0,
            cpu: "--".to_owned(),
            gpu: "--".to_owned(),
            endpoint: None,
            service: false,
        }
    }

    #[test]
    fn the_defaults_validate() {
        let rotation = options().validate().unwrap();
        assert_eq!(rotation.raw(), 0);
    }

    #[test]
    fn intervals_outside_the_range_are_rejected() {
        assert_eq!(parse_interval("8").unwrap(), 8);
        assert_eq!(parse_interval("33").unwrap(), 33);
        assert_eq!(parse_interval("60000").unwrap(), 60_000);
        assert!(parse_interval("7").is_err());
        assert!(parse_interval("60001").is_err());
        assert!(parse_interval("fast").is_err());
    }

    #[test]
    fn only_four_rotations_exist() {
        for value in 0u8..=3 {
            assert_eq!(parse_rotation(&value.to_string()).unwrap(), value);
        }
        assert!(parse_rotation("4").is_err());
        assert!(parse_rotation("-1").is_err());
    }

    #[test]
    fn labels_are_validated_before_the_device_is_touched() {
        let mut options = options();
        options.cpu = String::new();
        assert!(options.validate().unwrap_err().contains("empty"));
        options.cpu = "caf\u{e9}".to_owned();
        assert!(options.validate().unwrap_err().contains("ASCII"));
        options.cpu = "CPU".to_owned();
        options.rotation = 9;
        assert!(options.validate().unwrap_err().contains("rotation"));
    }
}
