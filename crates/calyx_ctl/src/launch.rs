use std::path::Path;
use std::process::{Child, Command as ProcessCommand, Stdio};
use std::time::{Duration, Instant};

use calyx_remote_protocol::{Command, DiscoveryInfo};

use crate::client::{discover, discovery_path, Client, CtlError};

/// Builds the editor (unless skipped), spawns it detached with remote control
/// enabled, and waits until the server answers a ping.
pub fn launch(
    project: &Path,
    port: u16,
    release: bool,
    skip_build: bool,
    timeout: Duration,
) -> Result<DiscoveryInfo, CtlError> {
    let repo_root = std::env::current_dir()
        .map_err(|error| CtlError::Transport(format!("cannot read current dir: {error}")))?;

    if !skip_build {
        let mut build = ProcessCommand::new("cargo");
        build.args(["build", "-p", "calyx_editor"]);
        if release {
            build.arg("--release");
        }
        eprintln!("Building editor (cargo build -p calyx_editor)...");
        let status = build.status().map_err(|error| {
            CtlError::Transport(format!("cargo build failed to start: {error}"))
        })?;
        if !status.success() {
            return Err(CtlError::Transport(format!(
                "cargo build -p calyx_editor failed with {status}"
            )));
        }
    }

    // A stale discovery file from a dead editor must not satisfy the readiness
    // poll below.
    let discovery_file = discovery_path(project);
    let _ = std::fs::remove_file(&discovery_file);

    // The editor outlives this process; it must not keep our caller's pipes
    // open, or a caller reading our output to its end would never finish.
    stop_std_handle_inheritance();
    let mut run = ProcessCommand::new("cargo");
    run.args(["run", "-p", "calyx_editor"]);
    if release {
        run.arg("--release");
    }
    run.arg("--").arg(project);
    run.current_dir(&repo_root)
        .env("CALYX_REMOTE_PORT", port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = run
        .spawn()
        .map_err(|error| CtlError::Transport(format!("failed to spawn editor: {error}")))?;
    eprintln!(
        "Spawned editor (cargo pid {}), waiting for remote server...",
        child.id()
    );

    let deadline = Instant::now() + timeout;
    loop {
        if Instant::now() > deadline {
            // The editor is useless to us now and would otherwise linger
            // holding the cargo target-dir lock.
            kill_process_tree(&mut child);
            return Err(CtlError::Timeout(format!(
                "editor did not become ready within {timeout:?}; check logs/ for errors"
            )));
        }
        std::thread::sleep(Duration::from_millis(250));

        // A bad project path (or a link failure) exits before the server is
        // reachable; without this the loop would wait out the full timeout
        // with the child's stderr already discarded.
        match child.try_wait() {
            Ok(Some(status)) => {
                return Err(CtlError::Transport(format!(
                    "editor exited with {status} before becoming ready; check logs/ for errors"
                )));
            }
            Ok(None) => {}
            Err(error) => {
                return Err(CtlError::Transport(format!(
                    "cannot poll editor process: {error}"
                )));
            }
        }

        let Ok(info) = discover(project) else {
            continue;
        };
        let Ok(mut client) = Client::connect(info.port, Duration::from_secs(1)) else {
            continue;
        };
        if client.call(Command::Ping, Duration::from_secs(2)).is_ok() {
            return Ok(info);
        }
    }
}

/// Stops this process's stdin, stdout and stderr from being inherited by
/// processes it spawns from now on. Windows hands every inheritable handle to
/// a child, whatever its own stdio is set to, and the editor `launch` spawns
/// runs on after we exit: holding our stdout, it would keep a pipe our caller
/// reads from open, and the caller would wait for its end as long as the
/// editor runs. A child given `Stdio::inherit` still gets its own copy.
fn stop_std_handle_inheritance() {
    #[cfg(windows)]
    for handle in win32::std_handles() {
        // SAFETY: `handle` is one of this process's standard handles.
        unsafe { win32::SetHandleInformation(handle, win32::HANDLE_FLAG_INHERIT, 0) };
    }
}

#[cfg(windows)]
mod win32 {
    pub type Handle = *mut core::ffi::c_void;

    pub const HANDLE_FLAG_INHERIT: u32 = 0x1;
    const STD_INPUT_HANDLE: u32 = -10i32 as u32;
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(std_handle: u32) -> Handle;
        pub fn SetHandleInformation(handle: Handle, mask: u32, flags: u32) -> i32;
        #[cfg(test)]
        pub fn GetHandleInformation(handle: Handle, flags: *mut u32) -> i32;
    }

    /// This process's standard handles that are set.
    pub fn std_handles() -> impl Iterator<Item = Handle> {
        [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE]
            .into_iter()
            // SAFETY: `GetStdHandle` only reads the process's handle table.
            .map(|which| unsafe { GetStdHandle(which) })
            .filter(|&handle| !handle.is_null() && handle as isize != -1)
    }
}

/// Kills `cargo run` together with the editor it spawned. Killing only the
/// cargo process would leave the editor running on Windows, which does not
/// take children down with their parent.
fn kill_process_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        let _ = ProcessCommand::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn launch_stops_its_standard_handles_from_being_inherited() {
        // Make them inheritable first, as a pipe from the caller may be.
        for handle in win32::std_handles() {
            // SAFETY: `handle` is one of this process's standard handles.
            unsafe {
                win32::SetHandleInformation(
                    handle,
                    win32::HANDLE_FLAG_INHERIT,
                    win32::HANDLE_FLAG_INHERIT,
                )
            };
        }
        stop_std_handle_inheritance();
        let mut checked = 0;
        for handle in win32::std_handles() {
            let mut flags = 0;
            // SAFETY: `handle` is valid and `flags` is a live u32.
            if unsafe { win32::GetHandleInformation(handle, &mut flags) } != 0 {
                assert_eq!(flags & win32::HANDLE_FLAG_INHERIT, 0);
                checked += 1;
            }
        }
        assert!(checked > 0, "the test process has standard handles");
    }
}
