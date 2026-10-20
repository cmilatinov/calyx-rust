use std::path::Path;
use std::process::{Child, Command as ProcessCommand, Stdio};
use std::time::{Duration, Instant};

use remote_protocol::{Command, DiscoveryInfo};

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
        build.args(["build", "-p", "editor"]);
        if release {
            build.arg("--release");
        }
        eprintln!("Building editor (cargo build -p editor)...");
        let status = build.status().map_err(|error| {
            CtlError::Transport(format!("cargo build failed to start: {error}"))
        })?;
        if !status.success() {
            return Err(CtlError::Transport(format!(
                "cargo build -p editor failed with {status}"
            )));
        }
    }

    // A stale discovery file from a dead editor must not satisfy the readiness
    // poll below.
    let discovery_file = discovery_path(project);
    let _ = std::fs::remove_file(&discovery_file);

    let mut run = ProcessCommand::new("cargo");
    run.args(["run", "-p", "editor"]);
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
