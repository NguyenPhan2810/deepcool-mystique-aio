//! Starting the daemon when nobody else has, and stepping away without stopping it.
//!
//! The app is self-contained in development without ever holding the daemon's lifetime:
//! if the control endpoint is absent, the daemon is spawned as a child, the endpoint is
//! awaited briefly, and the child handle is kept only for the exit check. Closing this
//! window drops that handle; it never terminates the process, so the panel stays held and
//! the stream keeps running.
//!
//! A daemon is stopped only by an explicit `shutdown` request from the UI, or by the
//! operator. The daemon's own single-instance lock makes a redundant spawn harmless: the
//! duplicate exits before it touches the socket or the panel.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

#[cfg(windows)]
const DAEMON_BINARY: &str = "mystique-daemon.exe";
#[cfg(not(windows))]
const DAEMON_BINARY: &str = "mystique-daemon";

/// How long to wait for the endpoint after a spawn. The daemon takes the lock, prepares
/// the socket and starts its runtime before it listens; a second is generous.
pub const STARTUP_WAIT: Duration = Duration::from_secs(3);

/// How often the endpoint is probed while waiting. Short enough to feel immediate, long
/// enough not to spin.
const PROBE: Duration = Duration::from_millis(50);

/// The one thing this module needs from a started process: whether it has exited. There
/// is deliberately no way to end it here; the daemon outlives this window by design.
pub(crate) trait ChildProcess: Send {
    /// Non-blocking. `Some(description)` once the process has exited.
    fn exited(&mut self) -> Option<String>;
}

impl ChildProcess for Child {
    fn exited(&mut self) -> Option<String> {
        match self.try_wait() {
            Ok(Some(status)) => Some(status.to_string()),
            Ok(None) => None,
            Err(error) => Some(format!("could not be queried: {error}")),
        }
    }
}

/// Which binary to run. Tests point this at a path that cannot be started.
#[derive(Debug, Clone)]
pub struct Launcher {
    program: PathBuf,
}

impl Launcher {
    /// The daemon beside this executable (`target/debug` for a workspace build), or a
    /// `PATH` lookup when there is no sibling binary.
    pub fn sibling() -> Self {
        let program = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join(DAEMON_BINARY)))
            .filter(|path| path.is_file())
            .unwrap_or_else(|| PathBuf::from(DAEMON_BINARY));
        Self { program }
    }

    #[cfg(test)]
    pub fn with_program(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        // The child inherits the environment, but the daemon reads its endpoint from a
        // flag, so an override has to travel as one.
        if let Ok(endpoint) = std::env::var("MYSTIQUE_ENDPOINT") {
            if !endpoint.trim().is_empty() {
                command.args(["--endpoint", endpoint.trim()]);
            }
        }
        command.stdin(Stdio::null()).stdout(Stdio::null());
        // A headless daemon has no terminal to talk to. Its banner and retry lines go to a
        // log file beside the settings, which is where a failed start is diagnosed; an
        // inherited stderr would open a console for every launch of a GUI app.
        command.stderr(match log_file() {
            Some(file) => Stdio::from(file),
            None => Stdio::null(),
        });
        command
    }

    fn start(&self) -> Result<Child, String> {
        self.command()
            .spawn()
            .map_err(|error| format!("could not start {}: {error}", self.program.display()))
    }
}

/// Per-user daemon log. Appended, because two lines per start is cheaper to keep than to
/// rotate; a runaway daemon is a separate problem.
fn log_file() -> Option<std::fs::File> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("XDG_STATE_HOME").map(std::path::PathBuf::from))
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|home| std::path::PathBuf::from(home).join(".local").join("state"))
        })?;
    let directory = base.join("mystique");
    std::fs::create_dir_all(&directory).ok()?;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("daemon.log"))
        .ok()
}

/// What this window has done about the daemon process, shared with the UI. Cloning is
/// cheap and every clone sees the same child handle.
#[derive(Clone, Default)]
pub struct SpawnRecord {
    inner: Arc<Mutex<Spawned>>,
}

#[derive(Default)]
struct Spawned {
    child: Option<Box<dyn ChildProcess>>,
    /// True only when *this* window started the process. Provenance, not a lifetime
    /// claim: the process is not stopped when this window closes.
    spawned: bool,
    error: Option<String>,
}

impl SpawnRecord {
    /// Ensures something is listening. Returns `Ok(true)` when this call started the
    /// daemon, `Ok(false)` when it was already running. On failure the reason is kept so
    /// the Connection section can name it.
    pub fn ensure(
        &self,
        launcher: &Launcher,
        available: &dyn Fn() -> bool,
        wait: Duration,
    ) -> Result<bool, String> {
        self.reap();
        if available() {
            self.set_error(None);
            return Ok(false);
        }

        let started = {
            let mut state = self.lock();
            if state.child.is_none() {
                match launcher.start() {
                    Ok(child) => {
                        state.child = Some(Box::new(child));
                        state.spawned = true;
                        true
                    }
                    Err(error) => {
                        state.error = Some(error.clone());
                        return Err(error);
                    }
                }
            } else {
                false
            }
        };

        match self.await_endpoint(available, wait) {
            Ok(()) => {
                self.set_error(None);
                Ok(started)
            }
            Err(error) => {
                self.set_error(Some(error.clone()));
                Err(error)
            }
        }
    }

    fn await_endpoint(&self, available: &dyn Fn() -> bool, wait: Duration) -> Result<(), String> {
        let deadline = Instant::now() + wait;
        loop {
            if available() {
                return Ok(());
            }
            if let Some(status) = self.exit_description() {
                return Err(format!(
                    "mystique-daemon exited with {status} before the control endpoint appeared"
                ));
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "the control endpoint did not appear within {} ms of starting mystique-daemon",
                    wait.as_millis()
                ));
            }
            std::thread::sleep(PROBE);
        }
    }

    fn exit_description(&self) -> Option<String> {
        self.lock().child.as_mut().and_then(|child| child.exited())
    }

    /// True when this window started the daemon. It is a note about provenance, not a
    /// claim on the process: closing the window does not stop it.
    pub fn spawned_by_ui(&self) -> bool {
        self.lock().spawned
    }

    /// The last spawn or startup failure, if any.
    pub fn last_error(&self) -> Option<String> {
        self.lock().error.clone()
    }

    /// Forgets a child that has already exited. A daemon stopped on request (or one that
    /// crashed) stops being this window's child, so the next `ensure` may start a fresh
    /// one instead of waiting on a dead handle.
    fn reap(&self) {
        let mut state = self.lock();
        let exited = state
            .child
            .as_mut()
            .and_then(|child| child.exited())
            .is_some();
        if exited {
            state.child = None;
            state.spawned = false;
        }
    }

    /// Drops this window's handle to a daemon it started. The process keeps running: its
    /// stdin and stdout are not connected to this window and its lifetime is not tied to
    /// it. Called from the app's exit hook.
    pub fn release(&self) {
        let mut state = self.lock();
        state.child.take();
        state.spawned = false;
    }

    fn set_error(&self, error: Option<String>) {
        self.lock().error = error;
    }

    fn lock(&self) -> MutexGuard<'_, Spawned> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[cfg(test)]
    fn install_child(&self, child: Box<dyn ChildProcess>, spawned: bool) {
        let mut state = self.lock();
        state.child = Some(child);
        state.spawned = spawned;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeChild {
        dropped: Arc<AtomicUsize>,
        exit: Option<String>,
    }

    impl FakeChild {
        fn running(dropped: &Arc<AtomicUsize>) -> Self {
            Self {
                dropped: Arc::clone(dropped),
                exit: None,
            }
        }
    }

    impl ChildProcess for FakeChild {
        fn exited(&mut self) -> Option<String> {
            self.exit.clone()
        }
    }

    impl Drop for FakeChild {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn an_running_endpoint_is_never_started_twice() {
        let launcher = Launcher::with_program("definitely-not-a-real-binary");
        let record = SpawnRecord::default();
        let started = record
            .ensure(&launcher, &|| true, Duration::from_millis(10))
            .expect("availability short-circuits the spawn");
        assert!(!started);
        assert!(!record.spawned_by_ui());
        assert!(record.last_error().is_none());
    }

    #[test]
    fn a_failed_spawn_is_reported_and_kept_for_the_ui() {
        let launcher = Launcher::with_program("definitely-not-a-real-binary");
        let record = SpawnRecord::default();
        let error = record
            .ensure(&launcher, &|| false, Duration::from_millis(10))
            .unwrap_err();
        assert!(error.contains("could not start"), "{error}");
        assert!(record
            .last_error()
            .unwrap_or_default()
            .contains("could not start"));
        assert!(!record.spawned_by_ui());
    }

    #[test]
    fn releasing_a_daemon_this_window_did_not_start_is_a_no_op() {
        let record = SpawnRecord::default();
        record.release();
        assert!(!record.spawned_by_ui());
    }

    #[test]
    fn the_exit_hook_drops_the_daemon_handle_and_never_terminates_it() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let record = SpawnRecord::default();
        record.install_child(Box::new(FakeChild::running(&dropped)), true);
        assert!(record.spawned_by_ui());

        record.release();

        assert_eq!(
            dropped.load(Ordering::SeqCst),
            1,
            "the handle is dropped so the OS can reclaim it"
        );
        assert!(
            !record.spawned_by_ui(),
            "there is no surviving claim on the process"
        );
        assert!(record.last_error().is_none());
    }

    #[test]
    fn an_exited_child_is_forgotten_so_a_stop_can_be_followed_by_a_start() {
        let record = SpawnRecord::default();
        record.install_child(
            Box::new(FakeChild {
                dropped: Arc::new(AtomicUsize::new(0)),
                exit: Some("exit status: 0".to_owned()),
            }),
            true,
        );

        // The dead handle must not be reused: a fresh spawn is attempted, and this fake
        // launcher is the observable proof that `ensure` reached it.
        let launcher = Launcher::with_program("definitely-not-a-real-binary");
        let error = record
            .ensure(&launcher, &|| false, Duration::from_millis(10))
            .unwrap_err();
        assert!(error.contains("could not start"), "{error}");
        assert!(!record.spawned_by_ui());
    }
}
