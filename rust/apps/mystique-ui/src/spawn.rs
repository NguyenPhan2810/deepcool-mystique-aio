//! One rule for every child process this GUI starts.
//!
//! Windows gives a console-subsystem child its own console window even when the parent is
//! a GUI app with every stream redirected. `sc.exe` and `reg.exe` are console-only, so a
//! status refresh or a startup-toggle read otherwise flashes a black window at the user.
//! The flag is applied at the call site so no spawn can forget it.

use std::process::Command;

pub fn quiet(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}
