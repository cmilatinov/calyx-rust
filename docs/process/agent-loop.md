---
inclusion: auto
---

# Agent Editor Loop - Remote Control

The editor embeds a remote control server that lets tools and AI agents drive
it programmatically: launch, load scenes, start/pause/stop simulation, inject
input, query and mutate world state as JSON, take screenshots, and shut down.
Use it to verify engine/editor changes end-to-end instead of asking a human to
click through the editor.

## Components

- `crates/calyx_remote_protocol` - the wire protocol (newline-delimited JSON over
  localhost TCP). Request/response envelopes, the `Command` enum, and
  `InputEventSpec` input scripts live here.
- `crates/calyx_editor/src/remote/` - the server. Enabled only when the editor starts with
  the `CALYX_REMOTE_PORT` environment variable set (`0` = ephemeral port); the
  editor is byte-for-byte inert otherwise. The bound port and editor pid are
  written to `<project>/.calyx/remote.json` (gitignored).
- `crates/calyx_ctl` - the CLI client. One subcommand per protocol command plus
  `launch` and `raw`. Prints the response payload as JSON on stdout; exit code
  `0` = ok, `1` = the editor answered with an error, `2` = transport/discovery
  failure.

## Quick start

```
cargo run -p calyx_ctl -- launch --project ./sandbox     # builds, spawns, waits; prints {port, pid}
calyx_ctl info                                           # session metadata + input_debug
calyx_ctl scene load sandbox/assets/game/scenes/arena.cxscene   # the tank gameplay scene
calyx_ctl play --scene sandbox/assets/game/scenes/arena.cxscene # plain `play` runs the startup scene (the showcase)
calyx_ctl focus-game                                     # REQUIRED before gameplay input
calyx_ctl wait --frames 3                                # let the Game tab publish its rect
calyx_ctl input key W --hold-frames 90
calyx_ctl object get Tank                                # assert world_transform changed
calyx_ctl screenshot --target game --out logs/shot.png
calyx_ctl pause
calyx_ctl step --frames 10                               # exact frame stepping, auto-pauses
calyx_ctl stop                                           # discards the play copy
calyx_ctl shutdown
```

`launch` returns once the editor answers a ping, but the editor keeps running
afterwards, so run it detached (a background job) rather than as a foreground
command whose output you wait on. It fails fast if the editor exits first
instead of waiting out the readiness timeout.

`calyx_ctl` binaries land in the cargo target dir (`target/debug/calyx_ctl.exe`
by default). **Invoke the built exe directly instead of `cargo run -p
calyx_ctl`** once it exists: the editor's background assembly build holds the
cargo lock for the shared target dir, and a `cargo run` client will block on it
for minutes.

## Command reference

Selectors: `object` accepts a game-object UUID or name (ambiguous names are an
error - use the UUID). `component` accepts a type UUID, a fully qualified type
name (`sandbox::tank::ComponentHealth`), or the bare type name
(`ComponentHealth`).

| CLI | Protocol | Notes |
|---|---|---|
| `ping`, `info`, `shutdown` | `ping`/`info`/`shutdown` | `info` includes `assemblies_loaded`, `is_simulating`, `object_count`, and an `input_debug` block |
| `play [--scene PATH]`, `pause`, `stop` | `play`/`pause`/`stop` | same semantics as the toolbar buttons, so a fresh `play` runs the project's configured startup scene, whichever scene is loaded; `--scene` runs that scene file instead (refused while a simulation exists); `stop` discards simulation edits. Anything that pauses, stops, or replaces the simulation - these commands, a successful `load_scene`, or the toolbar and scene loads in the editor UI - cancels an in-flight `step_frames` (that client gets a `cancelled` error) |
| `step --frames N` | `step_frames` | ensures simulation (a fresh start runs the startup scene, so `play --scene` first to step another scene), advances exactly N editor frames, pauses, then responds; only one step may be in flight at a time, and disconnecting mid-step pauses the simulation |
| `wait --frames N` / `wait --simulating BOOL` | `wait_frames` / client-side polling | |
| `scene load/save/state` | `load_scene`/`save_scene`/`get_scene_state` | paths resolve against the editor's working directory (repo root when using `launch`) |
| `prefab open PATH`, `prefab save`, `prefab close [--discard]` | `open_prefab`/`save_prefab`/`close_prefab` | prefab mode: `open` sets the scene aside and makes the `.cxprefab` the authoring scene the object, component and transform commands edit; `save` writes the prefab asset only; `close` refuses unsaved changes without `--discard`. `info` reports the open prefab under `prefab`. `play`, `step`, `scene load` and `scene save` are refused while a prefab is open |
| `prefab instantiate PATH`, `prefab update [PATH]` | `instantiate_prefab`/`update_prefab_instances` | `instantiate` adds a linked instance of the prefab at the scene root and returns its root `id`; `get_object` reports each instance object's `prefab_link`. `update` updates the scene's linked instances of that prefab (of every prefab without a path) from the saved asset as one undo step, keeping root transforms, object ids and overrides, and returns counts (`instances`, `added`, `removed`, `unlinked`, `kept_overrides`). Saving a prefab never updates instances by itself; `update` is refused while a prefab is open or a simulation scene exists |
| `asset open PATH [--save\|--discard\|--cancel]` | `open_asset` | opens the asset as a double-click in the content browser does, through the same open-asset dispatch: a scene loads into the scene view, a prefab opens in prefab mode, an animation graph is selected and the Animator tab comes to the front, a shader opens in the OS default app, and any other asset is selected for the inspector. Returns `outcome` (`opened`, `selected` or `prompted`), `selection`, `scene_file`, `scene_dirty` and `prefab`. When the open scene has unsaved changes, a scene open raises the Save / Don't Save / Cancel prompt; `--save`, `--discard` or `--cancel` answers it (`--save` is refused for a scene with no file), and without a flag the prompt stays open in the editor and the outcome is `prompted`. Scene and prefab opens are refused (`bad_request`) during play, and scene opens while a prefab is open |
| `objects`, `object get/create/delete` | `list_objects`/`get_object`/... | `get_object` returns local + world transforms and component list; `delete` clears the editor selection when it removes the selected object or one of its ancestors |
| `component get/set/add/types` | `get_component`/`set_component`/... | `set` replaces the whole component; fields missing from the JSON fall back to serde defaults, so round-trip `get` first and edit. `add` refuses a component the object already has (use `set`) and refuses `ComponentID`. `set` on `ComponentID` may rename but never change `id`, which indexes the object |
| `transform OBJ --position X Y Z [--rotation X Y Z] [--scale X Y Z] [--world]` | `set_transform` | rotation is XYZ Euler degrees |
| `select`, `pick X Y --space S --target T` | `select`/`pick` | pick uses the GPU object-id buffer of the chosen renderer |
| `focus-game [--release-grab]` | `focus_game` | activates the Game tab and grabs gameplay input focus |
| `input key/click/move/text/raw` | `inject_input` | see below |
| `screenshot --target window|game|viewport --out F` | `screenshot` | PNG; `window` includes the whole UI, `game`/`viewport` are the render textures. `game` works without the Game tab active: once the tab has been shown it is rendered every frame at the tab's size, and before that a game capture renders it on demand at the Scene viewport's size (or the window's). It fails with `bad_request` when the scene has no enabled camera, or when nothing has a size yet, naming `focus-game` |
| `raw '<json>'` | any | escape hatch for the full protocol |

Edits to the authoring scene (`object create/delete`, `component set/add`,
`transform`) are recorded in the editor's undo history like panel edits; edits
made while a simulation scene exists are discarded by `stop` and not recorded.

## Input injection

Synthetic input is injected through eframe's `raw_input_hook`, so it flows
through egui exactly like OS input and reaches both editor widgets and the
game `Input` path. `inject_input` responds only after the last scheduled event
has been delivered.

- **Call `focus-game` before gameplay input, then `wait --frames 2`.** Game
  input is gated on the Game panel holding focus and being visible; the panel
  rect used to resolve `game`-space coordinates is cached from the previous
  frame, so freshly activated tabs need a frame or two.
- Coordinate spaces: `window` = egui points in window space (what
  `query_panels` reports); `game`/`viewport` = 0..1 normalized inside that
  panel's image. Screenshots are physical pixels: multiply points by
  `pixels_per_point` from `query_panels`.
- `input raw` takes an `InputEventSpec` array for scripted sequences, e.g.
  `[{"type":"key_down","key":"W"},{"type":"wait","frames":30},{"type":"key_up","key":"W"}]`.
  Key names are egui names (`W`, `Space`, `Escape`, `ArrowLeft`).
- One bucket is allocated per frame a script spans, so a script may not span
  more than 36000 frames (ten minutes at 60 fps). `key_press` schedules its
  release `hold_frames` later but does not advance the cursor, so holds in one
  script overlap; use `wait` to sequence events.
- Keep hands off the real mouse/keyboard while injecting - OS input
  interleaves with synthetic events.

## Timing and determinism

- The editor runs on wall-clock delta time, so distances travelled per frame
  vary. Use tolerant assertions, or prefer `pause` + `step --frames N` for
  repeatable sequencing.
- Deferred commands (`step`, `inject_input`, `wait`, screenshots) complete on
  later frames; `calyx_ctl` scales its timeout with the frame count.
- The window must stay visible (not minimized) - frames and screenshots stall
  when Windows suppresses presents.

## Pitfalls

- **Modal dialogs freeze the loop.** `rfd` file dialogs block the update loop;
  queued commands only run after a human closes the dialog. Always use the
  path-based `scene load`/`scene save` instead of driving File > Open/Save via
  clicks.
- **Wait for `assemblies_loaded`** (`info`) before loading a scene that uses
  project components; loading earlier silently drops those components.
- **Scene paths**: `scene load` canonicalizes, but prefer paths relative to
  the repo root (the editor's working directory under `launch`).
- **Logs**: the editor writes `logs/calyx_editor_<timestamp>.log` in its
  working directory. `CALYX_LOG` (fallback `RUST_LOG`) filters, default
  `calyx_engine=info,calyx_editor=info,sandbox=info`. Remote commands are logged
  at debug level (`CALYX_LOG=calyx_editor=debug`).
- **Worktrees on Windows**: building in a git worktree under
  `.claude/worktrees/` can push native build scratch paths past the Windows
  path limit. Set `CARGO_TARGET_DIR` to a short path whose last component is `target`
  (e.g. `C:\Users\<you>\.calyx\target` - the editor test suite asserts the
  directory name) before building there; `calyx_ctl launch` honors it, and
  the editor's assembly build inherits it via the editor exe location.
- **No auth**: the server is localhost-only and trusts every connection. It is
  a development tool.
