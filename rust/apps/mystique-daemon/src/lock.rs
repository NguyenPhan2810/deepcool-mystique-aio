//! The single-instance lock.
//!
//! The device is exclusive: a second daemon would only fight the first for the USB
//! interface. A lock file held with an OS advisory lock gives a clear startup error and
//! is released automatically if the process dies, so a crash never needs a manual
//! cleanup. The file's contents are diagnostic only (the owning pid); the lock is the
//! OS lock, not the file's existence.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LockError {
    #[error("another mystique-daemon is already running (lock held on {0})")]
    AlreadyRunning(PathBuf),
    #[error("could not create the lock file {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub struct InstanceLock {
    _file: File,
    path: PathBuf,
}

impl InstanceLock {
    /// Creates the parent directory, opens the file and holds an exclusive lock for as
    /// long as the returned value lives.
    pub fn acquire(path: &Path) -> Result<Self, LockError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| LockError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)
            .map_err(|source| LockError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(LockError::AlreadyRunning(path.to_path_buf()))
            }
            Err(TryLockError::Error(source)) => {
                return Err(LockError::Io {
                    path: path.to_path_buf(),
                    source,
                })
            }
        }

        restrict(&file);

        // The contents are a diagnostic breadcrumb, never the lock itself.
        let mut handle = &file;
        let _ = handle.set_len(0);
        let _ = writeln!(handle, "pid={}", std::process::id());
        let _ = handle.flush();

        Ok(Self {
            _file: file,
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        // On Unix the socket and lock live in a per-user runtime directory; removing
        // the file on clean exit keeps that directory tidy. On Windows the file stays,
        // and the OS releases the lock with the handle.
        #[cfg(unix)]
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(unix)]
fn restrict(file: &File) {
    use std::os::unix::fs::PermissionsExt;

    let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
}

#[cfg(windows)]
fn restrict(_file: &File) {}

/// `%LOCALAPPDATA%\mystique\daemon.lock`, or the temp directory if that is unset.
#[cfg(windows)]
pub fn default_lock_path() -> Result<PathBuf, String> {
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        if !base.is_empty() {
            return Ok(PathBuf::from(base).join("mystique").join("daemon.lock"));
        }
    }
    Ok(std::env::temp_dir()
        .join("mystique-daemon")
        .join("daemon.lock"))
}

/// Next to the default socket: `$XDG_RUNTIME_DIR/mystique/daemon.lock` or the /tmp
/// fallback. The lock is per user, like the socket.
#[cfg(unix)]
pub fn default_lock_path() -> Result<PathBuf, String> {
    Ok(crate::endpoint::control_dir()?.join("daemon.lock"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_lock_is_refused_and_dropping_releases_the_first() {
        let dir = std::env::temp_dir().join(format!("mystique-lock-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("daemon.lock");

        let first = InstanceLock::acquire(&path).unwrap();
        assert_eq!(first.path(), path);
        let second = InstanceLock::acquire(&path);
        match second {
            Err(LockError::AlreadyRunning(reported)) => assert_eq!(reported, path),
            Err(other) => panic!("expected AlreadyRunning, got {other:?}"),
            Ok(_) => panic!("a second instance must not acquire the lock"),
        }

        drop(first);
        let third = InstanceLock::acquire(&path);
        assert!(third.is_ok(), "the lock must be released on drop");
        drop(third);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_lock_file_records_the_pid() {
        let dir = std::env::temp_dir().join(format!("mystique-lock-pid-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("daemon.lock");
        let lock = InstanceLock::acquire(&path).unwrap();

        // Windows' LockFileEx makes the locked range unreadable through a second
        // handle, so the breadcrumb is read there after the lock is released. Unix
        // permits the read while the lock is held, but removes the file on drop.
        #[cfg(unix)]
        let contents = std::fs::read_to_string(&path).unwrap();
        drop(lock);
        #[cfg(windows)]
        let contents = std::fs::read_to_string(&path).unwrap();

        assert!(contents.starts_with("pid="), "{contents}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
