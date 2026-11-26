# Standalone Player

A project runs outside the editor through
`calyx_engine::runtime::run_player(project_directory, plugin_main)`, which
owns the standalone window, GPU setup, frame loop, input, HUD and scene
rendering.

Code: `crates/calyx_engine/src/runtime/player.rs`,
`crates/calyx_engine/src/runtime.rs` (`ProjectRuntimeSettings`).

## What it does

- It reads the project's startup scene and window settings from
  `project.toml`, registers the linked project's component types before
  loading the scene, and runs until the window closes.
- The player binary links the project crate as an `rlib` (the project's
  `[lib]` builds both `cdylib` and `rlib`), so player and game code share one
  engine build and the [project assembly](project-assembly.md) DLL rules do
  not arise.
- It uses the same asset loader as the editor, so the project needs its
  assets locally, including any licensed imports.
- Player logs use `calyx_player_*.log` in the working directory's `logs/`
  folder.
- Game code reaches the player through `runtime::HostCommand`; the player
  closes its window on `Quit`.

## Adding a player to a project

A project adds a uniquely named Cargo binary and an `eframe` dependency for
the return type. Its entry point calls
`run_player(env!("CARGO_MANIFEST_DIR"), project_crate::plugin_main)` and
returns `eframe::Result<()>`; see `sandbox/src/game.rs` and
`demos/src/game.rs`.

Run the sandbox with `cargo run --release -p sandbox --bin game`, or the demo
collection with `cargo run --release -p demos --bin demos_game`. Measuring the
player is in [Agent Editor Loop](../../process/agent-loop.md#measuring-the-standalone-player).
