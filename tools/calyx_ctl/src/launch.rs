use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
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
    if let Some(russimp_bin) = find_russimp_bin_dir(&repo_root) {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![russimp_bin];
        paths.extend(std::env::split_paths(&path));
        if let Ok(joined) = std::env::join_paths(paths) {
            run.env("PATH", joined);
        }
    }
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
            let _ = child.kill();
            let _ = child.wait();
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

/// Locates the russimp (assimp) runtime DLL directory produced by the build
/// script, honoring a redirected `CARGO_TARGET_DIR`. Returns `None` when the
/// dependency is linked statically.
fn find_russimp_bin_dir(repo_root: &Path) -> Option<PathBuf> {
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root.join("target"));
    for profile in ["debug", "release"] {
        let pattern = target_dir
            .join(profile)
            .join("build")
            .join("russimp-sys*")
            .join("out")
            .join("dylib")
            .join("bin");
        let Some(pattern) = pattern.to_str().map(str::to_owned) else {
            continue;
        };
        if let Ok(paths) = glob::glob(&pattern) {
            for path in paths.flatten() {
                if path.is_dir() {
                    return Some(path);
                }
            }
        }
    }
    None
}
