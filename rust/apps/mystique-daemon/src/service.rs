//! The vendor service's state, queried with `sc.exe` on Windows.
//!
//! The panel is single-owner. On Windows the vendor `MystiqueService` may hold it, in
//! which case the daemon's open fails with access-denied until that service stops. The
//! daemon reports what `sc.exe` said verbatim; it does **not** stop or start the vendor
//! service. That is an owner decision, and it needs elevation.

use crate::environment::ServiceSnapshot;

pub const SERVICE_NAME: &str = "MystiqueService";

pub fn query() -> ServiceSnapshot {
    platform::query()
}

#[cfg(windows)]
mod platform {
    use std::process::{Command, Output};

    use super::{ServiceSnapshot, SERVICE_NAME};

    fn combined(output: &Output) -> String {
        let mut text = String::new();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stdout.trim().is_empty() {
            text.push_str(stdout.trim());
        }
        if !stderr.trim().is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(stderr.trim());
        }
        if text.is_empty() {
            text.push_str("(sc.exe produced no output)");
        }
        text
    }

    pub fn query() -> ServiceSnapshot {
        match Command::new("sc.exe")
            .args(["query", SERVICE_NAME])
            .output()
        {
            Ok(output) => {
                let text = combined(&output);
                ServiceSnapshot {
                    supported: true,
                    running: parse_running(&text),
                    output: text,
                }
            }
            Err(error) => ServiceSnapshot {
                supported: true,
                running: false,
                output: format!("could not run sc.exe: {error}"),
            },
        }
    }

    /// `sc query` prints a line like `        4  RUNNING`.
    fn parse_running(text: &str) -> bool {
        text.lines().any(|line| line.contains("RUNNING"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn running_is_read_from_the_state_line() {
            let text = "SERVICE_NAME: MystiqueService\n        STATE              : 4  RUNNING\n";
            assert!(parse_running(text));
        }

        #[test]
        fn a_stopped_or_missing_service_is_not_running() {
            assert!(!parse_running("STATE : 1  STOPPED"));
            assert!(!parse_running(
                "[SC] EnumQueryServicesStatus:OpenService FAILED 1060"
            ));
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use crate::environment::ServiceSnapshot;

    pub fn query() -> ServiceSnapshot {
        ServiceSnapshot {
            supported: false,
            running: false,
            output: "no vendor service on this platform; the daemon owns the device directly"
                .to_owned(),
        }
    }
}
