//! Where the control socket lives, per platform.
//!
//! Windows uses a named pipe at `\\.\pipe\mystique-control-v1`. Linux and other Unix
//! hosts use `$XDG_RUNTIME_DIR/mystique/control.sock`, falling back to
//! `/tmp/mystique-<uid>/control.sock`. Both are local to the machine; neither is ever a
//! TCP port.
//!
//! `--endpoint` overrides the location. On Unix the socket file is chmod 0600 after it is
//! bound. The directory is chmod 0700 only when this process created it — a shared
//! directory such as `/tmp` is never restricted, because that is not ours to change. A
//! stale socket left by a crash is removed only after a connect attempt proves nobody is
//! listening, and only when the path really is a socket.

#[cfg(windows)]
#[derive(Debug, Clone)]
pub struct Endpoint {
    pipe: String,
}

#[cfg(windows)]
impl Endpoint {
    pub fn default_endpoint() -> Result<Self, String> {
        Ok(Self {
            pipe: r"\\.\pipe\mystique-control-v1".to_owned(),
        })
    }

    pub fn from_option(value: Option<&str>) -> Result<Self, String> {
        match value {
            None => Self::default_endpoint(),
            Some(value) => {
                let value = value.trim();
                if value.is_empty() {
                    return Err("--endpoint must not be empty".to_owned());
                }
                if value.starts_with(r"\\.\pipe\") {
                    Ok(Self {
                        pipe: value.to_owned(),
                    })
                } else {
                    Ok(Self {
                        pipe: format!(r"\\.\pipe\{value}"),
                    })
                }
            }
        }
    }

    /// Nothing to prepare: a named pipe exists only while a server owns it.
    pub fn prepare(&self) -> Result<(), String> {
        Ok(())
    }

    pub fn cleanup(&self) {}

    pub fn display(&self) -> String {
        self.pipe.clone()
    }

    pub fn pipe_name(&self) -> &str {
        &self.pipe
    }
}

#[cfg(unix)]
#[derive(Debug, Clone)]
pub struct Endpoint {
    path: std::path::PathBuf,
}

#[cfg(unix)]
impl Endpoint {
    pub fn default_endpoint() -> Result<Self, String> {
        Ok(Self {
            path: control_dir()?.join("control.sock"),
        })
    }

    pub fn from_option(value: Option<&str>) -> Result<Self, String> {
        match value {
            None => Self::default_endpoint(),
            Some(value) => {
                let value = value.trim();
                if value.is_empty() {
                    return Err("--endpoint must not be empty".to_owned());
                }
                let path = std::path::PathBuf::from(value);
                if !path.is_absolute() {
                    return Err(format!(
                        "--endpoint must be an absolute path on this platform, got \"{value}\""
                    ));
                }
                Ok(Self { path })
            }
        }
    }

    /// Creates the directory, proves no live process owns the path, and removes only a
    /// provably stale socket.
    ///
    /// The directory is restricted to 0700 only when this call created it, or when it is
    /// the per-user control directory the endpoint owns by construction. `--endpoint` may
    /// point at a shared directory (`/tmp`, a test's temp dir); chmod on one of those is
    /// not permitted, and would be destructive if it were.
    pub fn prepare(&self) -> Result<(), String> {
        use std::os::unix::fs::{FileTypeExt, PermissionsExt};

        let parent = self.path.parent().ok_or_else(|| {
            format!(
                "{} has no parent directory; pass an absolute path",
                self.path.display()
            )
        })?;
        let existed = parent.exists();
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        let control_dir = control_dir().ok();
        if !existed || control_dir.as_deref() == Some(parent) {
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                .map_err(|error| format!("could not restrict {}: {error}", parent.display()))?;
        }

        match std::fs::symlink_metadata(&self.path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "could not inspect {}: {error}",
                    self.path.display()
                ))
            }
            Ok(metadata) => {
                if !metadata.file_type().is_socket() {
                    return Err(format!(
                        "{} exists and is not a socket; refusing to remove it",
                        self.path.display()
                    ));
                }
                match std::os::unix::net::UnixStream::connect(&self.path) {
                    Ok(_) => {
                        return Err(format!(
                            "another process is already listening on {}",
                            self.path.display()
                        ))
                    }
                    Err(_) => std::fs::remove_file(&self.path).map_err(|error| {
                        format!("could not remove {}: {error}", self.path.display())
                    })?,
                }
            }
        }
        Ok(())
    }

    pub fn cleanup(&self) {
        let _ = std::fs::remove_file(&self.path);
    }

    pub fn display(&self) -> String {
        self.path.display().to_string()
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

/// The per-user directory that holds the lock and, by default, the socket.
#[cfg(unix)]
pub(crate) fn control_dir() -> Result<std::path::PathBuf, String> {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        if !dir.is_empty() {
            return Ok(std::path::PathBuf::from(dir).join("mystique"));
        }
    }
    if let Some(uid) = current_uid() {
        return Ok(std::path::PathBuf::from(format!("/tmp/mystique-{uid}")));
    }
    Err(
        "XDG_RUNTIME_DIR is unset and the current uid could not be determined; \
         pass --endpoint explicitly"
            .to_owned(),
    )
}

#[cfg(target_os = "linux")]
fn current_uid() -> Option<u32> {
    // No libc and no `unsafe`: /proc/self/status is a text file with a `Uid:` line.
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("Uid:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(all(unix, not(target_os = "linux")))]
fn current_uid() -> Option<u32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn the_default_pipe_is_the_documented_one() {
        let endpoint = Endpoint::default_endpoint().unwrap();
        assert_eq!(endpoint.display(), r"\\.\pipe\mystique-control-v1");
    }

    #[cfg(windows)]
    #[test]
    fn a_bare_name_is_expanded_and_a_full_path_is_kept() {
        let bare = Endpoint::from_option(Some("mystique-test")).unwrap();
        assert_eq!(bare.display(), r"\\.\pipe\mystique-test");
        let full = Endpoint::from_option(Some(r"\\.\pipe\mystique-test")).unwrap();
        assert_eq!(full.display(), r"\\.\pipe\mystique-test");
        assert!(Endpoint::from_option(Some("   ")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn the_default_socket_is_named_control_sock() {
        let endpoint = Endpoint::default_endpoint().unwrap();
        assert!(endpoint.display().ends_with("control.sock"));
    }

    #[cfg(unix)]
    #[test]
    fn a_relative_socket_path_is_rejected() {
        assert!(Endpoint::from_option(Some("control.sock")).is_err());
        assert!(Endpoint::from_option(Some("  ")).is_err());
        assert!(Endpoint::from_option(Some("/tmp/mystique-test.sock")).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn a_socket_in_a_shared_directory_leaves_that_directory_alone() {
        use std::os::unix::fs::PermissionsExt;

        // `/tmp` is the case that failed: chmod on it is refused, and granting it would
        // be a system-wide change. A test temp dir behaves the same way.
        let shared = std::env::temp_dir();
        let before = std::fs::metadata(&shared).unwrap().permissions().mode();
        let path = shared.join(format!(
            "mystique-endpoint-shared-{}.sock",
            std::process::id()
        ));
        let endpoint = Endpoint::from_option(path.to_str()).unwrap();

        endpoint.prepare().unwrap();

        let after = std::fs::metadata(&shared).unwrap().permissions().mode();
        assert_eq!(before, after, "prepare must not chmod a shared directory");
        endpoint.cleanup();
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_this_process_creates_is_private() {
        use std::os::unix::fs::PermissionsExt;

        let root =
            std::env::temp_dir().join(format!("mystique-endpoint-dir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let endpoint = Endpoint::from_option(root.join("control.sock").to_str()).unwrap();

        endpoint.prepare().unwrap();

        let mode = std::fs::metadata(&root).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "a directory we create is ours to restrict");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn a_regular_file_on_the_socket_path_is_never_deleted() {
        let path = std::env::temp_dir().join(format!(
            "mystique-endpoint-file-{}.sock",
            std::process::id()
        ));
        std::fs::write(&path, b"not a socket").unwrap();
        let endpoint = Endpoint::from_option(path.to_str()).unwrap();

        let error = endpoint.prepare().unwrap_err();

        assert!(error.contains("not a socket"), "{error}");
        assert!(
            path.exists(),
            "prepare must not delete a file it did not create"
        );
        std::fs::remove_file(&path).unwrap();
    }
}
