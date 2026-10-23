# Toolchain

## The pinned nightly

`rust-toolchain.toml` pins one dated nightly with `rustfmt` and `clippy`. rustup installs it the
first time `cargo` runs in the repository, and CI installs the same one with `rustup toolchain
install`, so a developer machine and CI see the same compiler, lints and warnings.

Moving the pin is a deliberate change in its own pull request: change `channel`, run `agentic
local validate` with no warnings, and fix whatever the new nightly reports in that pull request.

## sccache on Windows (os error 206)

Builds through sccache can fail on Windows with:

```
sccache: error: failed to spawn Command { ... rustc.exe ... }
sccache: caused by: The filename or extension is too long. (os error 206)
```

Two changes meet here:

- Since July 2026 ([rust-lang/cargo#17258](https://github.com/rust-lang/cargo/pull/17258)),
  nightly Cargo uses the per-unit build-dir layout by default: each unit's artifacts live in
  `target/<profile>/build/<package>/<hash>/out`, and rustc gets one `-L dependency=...` per
  dependency instead of one for `target/<profile>/deps`. For `re_ui` in a worktree that is 423
  `-L` arguments and a 47,603-character command line, over Windows' 32,767-character limit.
- Cargo then passes the arguments in an `@argfile`. sccache 0.17 and later
  ([mozilla/sccache#2782](https://github.com/mozilla/sccache/pull/2782)) expand that file to
  decide whether the crate is cacheable, and spawn rustc with the expanded command line, which
  Windows refuses ([mozilla/sccache#2787](https://github.com/mozilla/sccache/issues/2787), fix
  proposed in [mozilla/sccache#2789](https://github.com/mozilla/sccache/pull/2789)).

Enabling Windows long paths does not help: it lifts the path limit, not the command-line limit.
Shortening `CARGO_TARGET_DIR` only buys margin: with a target directory of `C:\t` the same `re_ui` command is still
30,131 characters, and it grows with every dependency the workspace adds.

The repository's `.cargo/config.toml` keeps Cargo's legacy layout with
`[unstable] build-dir-new-layout = false`, so rustc gets a single `-L dependency=` and the command
line stays short. Nothing needs to change on a developer machine. Remove the setting once a
released sccache passes argfiles through to rustc, or if a later nightly drops the option (Cargo
then warns about an unknown unstable key, and the error above comes back on Windows).
