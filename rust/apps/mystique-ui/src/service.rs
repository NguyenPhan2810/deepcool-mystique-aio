//! `MystiqueService` start and stop, with `sc.exe`.
//!
//! These are the only two operations this window still performs on its own. The device is
//! single-owner: while the vendor service runs, the daemon's open returns access-denied,
//! and the UI offers a stop button that requires elevation. Failures are surfaced
//! verbatim, including `sc.exe`'s own text.
//!
//! The service's *current* state is not queried here any more: the daemon reports
//! `service_running` and the `sc.exe query` text in every `DeviceState`. A local query
//! remains only to wait for a start or stop to take effect.

/// The vendor service only exists on Windows. The rest of the module still exports
/// [`stop`] and [`start`] everywhere so the command layer does not need its own `cfg`;
/// on Linux they answer with a typed "unsupported" message.
#[cfg(windows)]
pub const SERVICE_NAME: &str = "MystiqueService";

/// Stops the service and waits up to five seconds for it to leave `RUNNING`.
///
/// On success returns the verbatim `sc.exe` output. On failure returns a message that
/// already contains the verbatim output, including the access-denied case.
pub fn stop() -> Result<String, String> {
    platform::stop()
}

/// Starts the service and waits up to five seconds for it to report `RUNNING`.
pub fn start() -> Result<String, String> {
    platform::start()
}

#[cfg(windows)]
mod platform {
    use std::process::{Command, Output};
    use std::time::Duration;

    use super::SERVICE_NAME;

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

    fn exit_label(output: &Output) -> String {
        match output.status.code() {
            Some(code) => format!("exit code {code}"),
            None => "terminated by signal".to_owned(),
        }
    }

    /// `sc query` prints a line like `        4  RUNNING`. A STOP_PENDING service is
    /// still not stopped, but it is also not holding the interface for long; the stop
    /// path polls until RUNNING disappears.
    fn running() -> bool {
        let mut command = Command::new("sc.exe");
        command.args(["query", SERVICE_NAME]);
        crate::spawn::quiet(&mut command);
        match command.output() {
            Ok(output) => {
                let text = combined(&output);
                text.lines().any(|line| line.contains("RUNNING"))
            }
            Err(_) => false,
        }
    }

    pub fn stop() -> Result<String, String> {
        let mut command = Command::new("sc.exe");
        command.args(["stop", SERVICE_NAME]);
        crate::spawn::quiet(&mut command);
        let output = command
            .output()
            .map_err(|error| format!("could not run sc.exe: {error}"))?;
        let text = combined(&output);
        if !output.status.success() {
            return Err(format!(
                "sc.exe stop {SERVICE_NAME} failed ({}):\n{text}",
                exit_label(&output)
            ));
        }

        for _ in 0..20 {
            if !running() {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        Ok(text)
    }

    pub fn start() -> Result<String, String> {
        let mut command = Command::new("sc.exe");
        command.args(["start", SERVICE_NAME]);
        crate::spawn::quiet(&mut command);
        let output = command
            .output()
            .map_err(|error| format!("could not run sc.exe: {error}"))?;
        let text = combined(&output);
        if !output.status.success() {
            return Err(format!(
                "sc.exe start {SERVICE_NAME} failed ({}):\n{text}",
                exit_label(&output)
            ));
        }

        for _ in 0..20 {
            if running() {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        Ok(text)
    }

    #[cfg(test)]
    mod tests {
        fn parses(text: &str) -> bool {
            text.lines().any(|line| line.contains("RUNNING"))
        }

        #[test]
        fn running_is_read_from_the_state_line() {
            let text = "SERVICE_NAME: MystiqueService\n        TYPE               : 10 WIN32_OWN_PROCESS\n        STATE              : 4  RUNNING\n";
            assert!(parses(text));
        }

        #[test]
        fn stopped_is_not_running() {
            let text = "SERVICE_NAME: MystiqueService\n        STATE              : 1  STOPPED\n";
            assert!(!parses(text));
        }

        #[test]
        fn missing_service_is_not_running() {
            let text = "[SC] EnumQueryServicesStatus:OpenService FAILED 1060:\n\nThe specified service does not exist as an installed service.";
            assert!(!parses(text));
        }
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn stop() -> Result<String, String> {
        Err("service control is Windows-only in this app".to_owned())
    }

    pub fn start() -> Result<String, String> {
        Err("service control is Windows-only in this app".to_owned())
    }
}
