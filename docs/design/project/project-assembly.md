# Project Assembly

A game project compiles to a dynamic library, the project assembly, that the
editor builds, loads and reloads. Its `plugin_main` registers the project's
types. The assembly statically links its own copy of `calyx_engine`, and that
one fact drives the rules below: engine types must match across the boundary,
and every engine `static` exists twice.

Code: `crates/calyx_editor/src/project_manager.rs` (`ProjectManager`),
`crates/calyx_engine/src/reflect/type_registry.rs`
(`TypeRegistry::register_crate`, `unload_plugins`),
`crates/calyx_engine/src/scene/scene_manager.rs` (`suspend_scenes`,
`resume_scenes`), `crates/calyx_engine_feature_pins/`.

## One engine build on both sides

- Engine types cross the DLL boundary keyed by `TypeId`: Legion's component
  storages, reflection, resource and trait-metadata downcasts all compare
  `TypeId`s. A type crosses with its ordinary Rust layout, since both sides
  share the engine build.
- The DLL is built in a separate `cargo build --lib`, so engine's
  dependencies must resolve to the same features there as in the editor
  build, or the DLL gets another engine build with other `TypeId`s.
- `crates/calyx_engine_feature_pins` pins the editor's features, and under
  `[build-dependencies]` the host-side features a `cargo test --workspace`
  adds, since the editor's tests load the sandbox assembly too. Its
  `Cargo.toml` explains each pin.
- The workspace's `[profile.dev.build-override]` keeps full debug info for
  build scripts and proc macros, so the two builds compile engine's proc
  macros the same way.
- `crates/calyx_editor/tests/project_assembly_features.rs` fails when the
  features drift, and the editor logs an error when it loads an assembly built
  against another engine.
- `crates/calyx_editor/tests/project_assembly_type_ids.rs` loads the sandbox
  assembly and downcasts its trait metadata and engine-typed fields in the
  editor.

## Statics

The DLL's own copy of `calyx_engine` and its dependencies also means its own
copy of every `static` and `thread_local!` in them. Code that reaches one from
the DLL sees the DLL's copy, not the editor's. The ones that matter:

- **Legion's entity counter** (`NEXT_ENTITY`): the DLL's starts over and would
  hand out entities the editor already did, so a `Scene` keeps a pointer to
  the allocator of the engine build that created it and spawns every object,
  from either side, with it.
- **The prefab id remap** (`RemapScope`, a thread-local read by
  `GameObjectRef` deserialization): each component type enters it in its own
  code through `ComponentInstance::deserialize_remapped`, so a project
  component sees the remap the editor's prefab instantiation asked for.
- **The `log` logger**: `TypeRegistry::register_crate` installs the logger the
  editor recorded in `begin_plugin_load`, so DLL log records reach the
  editor's log.

Safe as they are:

- the scene id counter (`SceneId`): the DLL's copy starts over, but the only
  scenes the particle simulation binds, the editor's play copy and the
  player's scene, are built by the editor or player's own engine build;
- Legion's `ID_CLONE_MAPPINGS` (only `World::clone_from`, which the engine
  never calls), `COMPONENT_VERSION` (only Legion's `maybe_changed` filter,
  unused; the scene's change set tracks writes) and `WORLD_ID_COUNTER` (keys
  Legion query caches, and the engine builds a fresh query per use);
- `warn_once` keys (a warning can show once per copy);
- `inventory` registrations (each copy collects its own, which is how
  `register_crate` finds only the DLL's types).

## Reload

Reloading the project DLL after a rebuild unloads the previous one, and
everything its code backs must go first: Legion storages of project
components (their drop, clone and serialize code is the DLL's), the
`allocate_entity` pointer of a scene the DLL created, the DLL's type
registrations and the component binders made from them.

So `ProjectManager` loads the new DLL, then, while the old one is still
loaded, has `SceneManager::suspend_scenes` snapshot and empty every scene it
reaches, calls `TypeRegistry::unload_plugins` (which resets the registry to
the editor's own types, dropping types the new build removed too) and
rebuilds the component registry from it. It then registers the new DLL's
types, unloads the old DLL and rebuilds each scene from its snapshot with
`SceneManager::resume_scenes`. A reload:

- keeps the authoring scene with its unsaved edits, its file and dirty state,
  the selection, and the undo and redo history, which are all serialized;
- keeps each cached scene asset and the scene manager's default scene, even
  one the cache has evicted, rebuilt in place behind the same reference;
  prefabs are serialized data and stay as they are;
- stops a running or paused simulation and drops its scene, as Stop does;
  Play starts a fresh one;
- commits pending inspector edits before the reload and takes the inspector's
  baselines again after it;
- drops components whose type the new build removed, or that no longer
  deserialize, from the rebuilt scenes, so saving afterwards writes the scene
  without them (`scene/dropped_components.rs` reports them in one warning);
- does not cover resources or asset types project code adds to the editor;
  the sandbox adds none.

## Loading on Linux

The editor loads the project DLL with `libloading`, which hands `dlopen` and
`dlsym` NUL-terminated strings (`sharedlib`, used before, passed unterminated
ones on Linux, so no assembly ever loaded there). Every load is of a fresh
shadow copy under its own folder, so on Linux `dlopen` returns a new handle
for each build, and dropping the previous one `dlclose`s and unmaps it: after
a reload only the new copy is mapped. Linux runs `calyx_editor`'s assembly
reload test like Windows does.
