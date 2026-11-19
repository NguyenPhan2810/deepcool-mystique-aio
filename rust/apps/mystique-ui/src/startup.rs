//! Start-with-system and start-minimized, implemented where each OS expects it.
//!
//! Windows uses the per-user `Run` key, so no elevation is needed and the entry is
//! visible in Task Manager's Startup tab like any other application. Linux writes an
//! XDG autostart `.desktop` file. The window is hidden at launch when the saved command
//! carries `--minimized`; the tray icon is the only visible surface in that case.
//!
//! The registry/file entry is the single source of truth: reading it back means the
//! toggle cannot drift from reality if someone removes the entry by hand.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StartupState {
    pub enabled: bool,
    pub minimized: bool,
    /// False on platforms without an implementation; the UI disables the toggles.
    pub supported: bool,
}

/// Only the fallback platform builds an "unsupported" value.
#[cfg(not(any(windows, target_os = "linux")))]
impl StartupState {
    const fn unsupported() -> Self {
        Self {
            enabled: false,
            minimized: false,
            supported: false,
        }
    }
}

/// The command line written into the startup entry. Quoted because the install path
/// contains spaces.
fn launch_command(minimized: bool) -> Result<String, String> {
    let exe =
        std::env::current_exe().map_err(|error| format!("could not find this exe: {error}"))?;
    let quoted = format!("\"{}\"", exe.display());
    Ok(if minimized {
        format!("{quoted} --minimized")
    } else {
        quoted
    })
}

#[cfg(windows)]
mod platform {
    use std::process::Command;

    use super::StartupState;

    const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "MystiqueControl";

    pub fn read() -> StartupState {
        let mut command = Command::new("reg");
        command.args(["query", RUN_KEY, "/v", VALUE]);
        crate::spawn::quiet(&mut command);
        let output = command.output();
        let Ok(output) = output else {
            return StartupState {
                enabled: false,
                minimized: false,
                supported: true,
            };
        };
        let enabled = output.status.success();
        let text = String::from_utf8_lossy(&output.stdout);
        StartupState {
            enabled,
            minimized: enabled && text.contains("--minimized"),
            supported: true,
        }
    }

    pub fn write(enabled: bool, minimized: bool) -> Result<(), String> {
        let status = if enabled {
            let command_line = super::launch_command(minimized)?;
            let mut command = Command::new("reg");
            command.args([
                "add",
                RUN_KEY,
                "/v",
                VALUE,
                "/t",
                "REG_SZ",
                "/d",
                &command_line,
                "/f",
            ]);
            crate::spawn::quiet(&mut command);
            command.status()
        } else {
            let mut command = Command::new("reg");
            command.args(["delete", RUN_KEY, "/v", VALUE, "/f"]);
            crate::spawn::quiet(&mut command);
            command.status()
        };
        match status {
            Ok(status) if status.success() || !enabled => Ok(()),
            Ok(status) => Err(format!("reg exited with {status}")),
            Err(error) => Err(format!("could not run reg: {error}")),
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::fs;
    use std::path::PathBuf;

    use super::StartupState;

    fn entry_path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        Some(base.join("autostart").join("mystique-control.desktop"))
    }

    pub fn read() -> StartupState {
        let Some(path) = entry_path() else {
            return StartupState {
                enabled: false,
                minimized: false,
                supported: true,
            };
        };
        match fs::read_to_string(&path) {
            Ok(text) => StartupState {
                enabled: true,
                minimized: text.contains("--minimized"),
                supported: true,
            },
            Err(_) => StartupState {
                enabled: false,
                minimized: false,
                supported: true,
            },
        }
    }

    pub fn write(enabled: bool, minimized: bool) -> Result<(), String> {
        let path = entry_path().ok_or_else(|| "no XDG config directory".to_owned())?;
        if !enabled {
            match fs::remove_file(&path) {
                Ok(()) => return Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(format!("could not remove {}: {error}", path.display())),
            }
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        }
        let command = super::launch_command(minimized)?;
        let body = format!(
            "[Desktop Entry]\nType=Application\nName=MYSTIQUE Control\nExec={command}\nX-GNOME-Autostart-enabled=true\n"
        );
        fs::write(&path, body)
            .map_err(|error| format!("could not write {}: {error}", path.display()))
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    use super::StartupState;

    pub fn read() -> StartupState {
        StartupState::unsupported()
    }

    pub fn write(_enabled: bool, _minimized: bool) -> Result<(), String> {
        Err("start-with-system is not implemented on this platform".to_owned())
    }
}

pub fn read() -> StartupState {
    platform::read()
}

pub fn write(enabled: bool, minimized: bool) -> Result<(), String> {
    platform::write(enabled, minimized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launch_line_quotes_the_path_and_carries_the_flag() {
        let plain = launch_command(false).unwrap();
        let minimized = launch_command(true).unwrap();
        assert!(plain.starts_with('"') && plain.ends_with('"'));
        assert_eq!(minimized, format!("{plain} --minimized"));
    }
}
