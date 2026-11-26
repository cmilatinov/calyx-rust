# Prefab Mode

Prefab mode opens a `.cxprefab` asset as the authoring scene under its own
`SceneDocument`, edits it with every ordinary editor tool, and saves only the
prefab asset.

Code: `crates/calyx_editor/src/prefab_mode/` (`PrefabMode` in its root
module, `editing.rs`, `environment.rs`, `ui.rs`).

## Opening and closing

- A prefab opens by double-clicking it in the content browser or from its Open
  Prefab context action, both through the editor's asset-open dispatch
  (`asset_open/`).
- The open scene is set aside as a snapshot with its document, file and
  selection, and comes back unchanged when the prefab closes.
- Save writes the prefab asset only (one top-level object, the prefab root).
  Scene instances change only when the explicit Update Prefab Instances action
  runs ([Prefab Links](../scene/prefab-links.md#updating)).
- Play, and opening or saving scenes, are refused while a prefab is open.
  Closing or quitting with unsaved prefab changes prompts.

## Look

- The scene view shows a blue backdrop in prefab mode (`PREFAB_MODE_BACKDROP`).
- It lights the prefab with a view-only environment (`environment.rs`): the
  built-in studio HDRI `assets/skyboxes/prefab_studio.hdr`, used for
  image-based lighting without being drawn and passed to the renderer as
  `SceneRendererOptions::environment`. Its bright spot at 45 degrees gives the
  prefab a shading direction, with no separate key light.
- Nothing of it is saved into the prefab.

## State

- `PrefabMode` is the one `EditorAppState` field holding the open prefab, the
  pending unsaved-changes prompt and the loaded studio HDRI.
- `EditorAppState::prefab_mode_mut` pairs it with the game, scene document,
  selection and project path to open, save and close prefabs.
- `editing.rs` swaps the prefab in and the set-aside scene back, and `ui.rs`
  holds the banner and the unsaved-changes prompt.
