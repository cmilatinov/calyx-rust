# Calyx Architecture

Calyx is a 3D game engine and editor written in Rust, built for a multiplayer
tank game. This is the module map: what each crate and module is for, the
seams that hold between them, and where each feature's design doc lives. The
detail is in the linked docs and the code's rustdoc.

## Layout rule

Modules are `foo.rs` plus an optional `foo/`, never `mod.rs`; tests always
live in `foo/tests.rs`, never inline and never in `*_tests.rs`.

Older modules still have `mod.rs` files, inline `mod tests` and `*_tests.rs`
files; new code follows the rule.

## Workspace crates

The crates live under `crates/`, each named with a `calyx_` prefix; the two
game projects, `sandbox` and `demos`, sit at the repository root.

- **`calyx_engine`** (library): the runtime: assets, ECS components, scenes,
  rendering, physics, networking, reflection and the standalone player. Its
  modules are mapped below.
- **`calyx_engine_derive`** (proc-macro): `#[derive(Component)]` (the
  type-erased `ComponentInstance` impl), `#[derive(Reflect)]`,
  `#[derive(TypeUuid)]`, `#[derive(Resource)]`, `#[derive(PostProcessEffect)]`
  (bridges a component's `BlendEffect` to the reflected post-process effect
  trait), `#[derive(DeserializeWithContext)]`, `#[reflect_trait]`,
  `impl_reflect_value!` and `impl_extern_type_uuid!`. `fq.rs` holds
  fully-qualified path tokens for the generated code.
- **`calyx_engine_feature_pins`** (library, no code): pins the features of
  engine's dependencies to what the editor resolves, so a project assembly
  links the same engine build; see
  [Project Assembly](../design/project/project-assembly.md).
- **`calyx_shader`** (library): the WGSL preprocessor, a pure source-to-source
  pass with no engine dependency (`//#include`, `//#param`, `//#binding`),
  returning a `SourceMap` and naming the file and line of every `ShaderError`.
  Its crate rustdoc documents the directive language; its tests run on the
  in-memory `MemoryLoader`.
- **`calyx_editor`** (binary `editor`): the egui + eframe editor. Its modules
  are mapped below.
- **`calyx_remote_protocol`** (library): the newline-delimited JSON protocol
  between the editor's remote control server and its clients: request and
  response envelopes and every `Command`.
- **`calyx_ctl`** (binary): the command-line client of that protocol, which
  launches the editor and drives it from scripts and agents; see
  [Agent Editor Loop](../process/agent-loop.md).
- **`calyx_project`** (library): the project manifest, `project.toml`
  (`Project`: name, startup scene, runtime and inspector settings), and new
  project scaffolding: `project.toml`, a `Cargo.toml` from
  `assets/cargo.template` and an empty `assets/lib.rs`.
- **`calyx_unity_import`** (binary): imports a Unity Asset Store pack into a
  Calyx project; see [Unity Import](../design/tools/unity-import.md).
- **`calyx_model_gen`** (binary): generates the sandbox's low-poly clay models
  in headless Blender; see [Model Generator](../design/tools/model-gen.md).
- **`calyx_icon_atlas`** (binary): packs SVG or PNG icons into one PNG atlas
  and JSON manifest; its `README.md` documents the options.
- **`sandbox`** (cdylib + rlib + binary `game`): the tank game project; see
  below.
- **`demos`** (cdylib + rlib + binary `demos_game`): a project of standalone
  demo scenes showing what the engine renders; its `README.md` lists them.

## `calyx_engine` modules

- **`assets`**: the `Asset` trait, the `AssetRegistry` with its file watcher
  and `.meta` sync, `AssetRef<T>` lazy references, and the asset types Mesh,
  Texture, Material, Animation, AnimationGraph, Skybox and PostProcessProfile.
  `assets/import` turns FBX, OBJ, glTF and GLB into an imported scene. See
  [Asset Files](../design/assets/asset-files.md) and
  [Model Import](../design/assets/model-import.md).
- **`background`**: `Background`, the shared thread-pool task executor
  resource, which runs the editor's assembly builds among other tasks.
- **`class_registry`**: `ComponentRegistry`, one prototype per reflected
  component type for type-erased construction, binding and serialization, and
  each type's lifecycle hooks as reflected functions.
- **`component`**: the `Component` trait, the lifecycle hook traits
  (`ComponentStart`, `ComponentUpdate`, `ComponentReset`, `ComponentHud`),
  `ComponentRef<T>`, and the built-in components: transform, cameras, meshes,
  lights, physics bodies, animation, particles, terrain, water, foliage,
  grass and the post-process volumes and effects. See
  [Components](../design/scene/components.md).
- **`context`**: the layered contexts: `RegistryContext` (asset, type and
  component registries), `AssetContext` (those plus the render context) and
  `GameContext` (assets, `SceneManager`, `ResourceMap` and the particle
  simulation), whose `update_scene` and `prepare_scene` run a frame.
- **`core`**: `Ref<T>` (`Arc<RwLock<T>>` with a UUID), `ReadOnlyRef<T>`,
  `WeakRef<T>` and `Time` (`log_registry.rs` is commented out).
- **`error`**: the `BoxedError` and `DynError` aliases.
- **`ext`**: extension traits for egui and nalgebra types, and `lerp`
  impls.
- **`input`**: `Input` over egui's input state, and `ActionMap`, named actions
  and axes over key and pointer bindings. A project ships its bindings as the
  `input.cxinput` asset (`PROJECT_ACTION_MAP_NAME`), which
  `GameContext::new` and `HeadlessSceneRunner::from_scene` follow, hot reloads
  included; the default map, and a project without one, binds nothing.
- **`logging`**: `Log` and `DefaultLogger` (timestamped log files and an
  in-memory history the editor's terminal shows), and `warn_once`.
- **`math`**: `Transform`, `Curve` and `Gradient` (keys over time, evaluated on
  the CPU and, through `render`, the GPU), Bezier and Catmull-Rom helpers, and
  the geometry behind terrain (`heightfield`) and water (`water`,
  `waterline`).
- **`net`**: Renet client/server multiplayer with the server as the only
  relay, per-peer network id ranges, transform sync and join snapshots. See
  [Networking](../design/net/networking.md).
- **`physics`**: Rapier3D at a fixed 1/60 s step, collision groups, ray and
  shape queries, and the frame's collision and contact-force events. See
  [Physics](../design/engine/physics.md).
- **`profiler`**: the frame profiler: puffin CPU scopes (`profile_scope!`,
  `profile_function!`), GPU timings per pass (`GpuScope`), and a
  `ProfileReport` over recent frames; the module rustdoc states its rules,
  and `calyx_ctl profiler read` shows it
  ([Agent Editor Loop](../process/agent-loop.md#profiling)).
- **`reflect`**: the `Reflect` trait, `TypeInfo`, `TypeRegistry` (keyed by
  type UUID, downcasting by `TypeId`) and trait metadata. See
  [Reflection](../design/engine/reflection.md).
- **`render`**: the wgpu renderer. `RenderContext` (eframe or headless),
  `SceneRenderer` with its deferred G-buffer path, the `DrawCollection`, the
  `Shader` asset and `AssetShaderLoader`, lights, shadows, particles, terrain,
  water, foliage, the post-processing chain (`postprocess/`) and the editor
  overlays (`overlay/`: grid, gizmos and selection outline composited after
  the chain). See the rendering docs below.
- **`resource`**: `ResourceMap`, type-keyed runtime resources (`Time`,
  `Background`, `Network`, `InputState`, `PhysicsConfiguration`,
  `HostCommands` and any a project adds).
- **`runtime`**: runtime setup shared by the player and the editor: project
  runtime settings, the GPU device both open, window icons, per-frame
  bookkeeping, `HostCommand`, and `run_player`. See
  [Standalone Player](../design/project/standalone-player.md).
- **`scene`**: `Scene` (a `SceneId`, the ECS world, physics, and the scene
  graph, object store, transform cache, change journal and disabled-component
  map it delegates to), `SceneManager` for load, save and simulation, scene
  and prefab assets with id remapping, `SceneView` (a read-only view of
  objects and hierarchy) and the HUD build. See
  [Component Enabled State](../design/scene/component-enabled-state.md) and
  [Prefab Links](../design/scene/prefab-links.md).
- **`test_support`**: headless helpers for engine and integration tests,
  such as `HeadlessSceneRunner`.
- **`ui`**: the backend-neutral runtime UI the HUD is built with: layout,
  styles, text, paint commands, `UiRuntime` and the `UiBackend` boundary with
  its egui backend. See [HUD / Runtime UI Architecture](../design/hud-ui/architecture.md).
- **`utils`**: `TypeUuid`, `ContextSeed`, the `impl_builder_fn!` macro,
  `lib_file_name` (a library's platform file name) and other small
  utilities.

### Rendering docs

- [G-Buffer](../design/rendering/gbuffer.md): the geometry pass and deferred lighting.
- [Draw Collection and Static Batches](../design/rendering/draw-collection.md)
- [Lights](../design/rendering/lights.md): the light buffer and falloff.
- [Shadow Mapping](../design/rendering/shadows.md)
- [PBR Materials](../design/rendering/pbr-materials.md)
- [Shaders](../design/rendering/shaders.md): loading, compute, hot reload and the shader folder layout.
- [Post-Processing Architecture](../design/post-processing/architecture.md)
- [Particles](../design/rendering/particles.md), [Terrain](../design/rendering/terrain.md),
  [Water](../design/rendering/water.md), [Grass](../design/rendering/grass.md),
  [Foliage](../design/rendering/foliage.md)

## `calyx_editor` modules

- **`lib.rs`**: `EditorApp`, its frame loop, and `EditorAppState`, the
  state panels share; `main.rs` only runs it.
- **`panel/`**: the `Panel` trait (name, icon, ui) and the panels: Viewport,
  Game, Inspector, Scene Hierarchy, Content Browser, Terminal, Animator and
  Profiler (frame, CPU and GPU time of recent frames and the scope breakdown
  over a window).
- **`inspector/`**: `InspectorRegistry` with type inspectors (bool, number,
  string, color, transform, vector, curve, gradient, asset and game object
  references, collider, rigid body, UUID, blend mode, local volume shape,
  particle spawn shape), asset inspectors (Material, Shader, Prefab, Scene,
  AnimationGraph, PostProcessProfile), the component inspector, the edit
  tracker (undo baselines of shown components) and the pinned component order.
- **`widgets/`**: tabs, the file button, the list widget, the curve and
  gradient editors, and the `ThumbnailService` that renders asset thumbnails
  in the background.
- **`asset_open/`**: the one dispatch every asset open goes through
  (double-click, Enter, the context menu's Open, `calyx_ctl asset open`):
  `AssetOpener`s registered by asset type, and the unsaved-scene prompt.
- **`remote/`**: the remote control server `calyx_ctl` drives, enabled only
  when `CALYX_REMOTE_PORT` is set; see [Agent Editor Loop](../process/agent-loop.md).
- **`scene_document.rs`**, **`scene_edit.rs`**, **`scene_history.rs`**,
  **`scene_file.rs`**, **`scene_autosave.rs`**: the authoring scene's
  document: undo history, dirty state, file identity, saving and recovery
  autosave, written through `SceneEdit`.
- **`prefab_mode/`**: editing a `.cxprefab` as the authoring scene; see
  [Prefab Mode](../design/editor/prefab-mode.md).
- **`prefab_instances.rs`**: instantiating linked prefabs and the Update
  Prefab Instances action; see [Prefab Links](../design/scene/prefab-links.md).
- **`project_manager.rs`**: `ProjectManager`: loads the project and its
  runtime settings, starts the simulation, queues assembly builds on the
  background executor, and loads and reloads the project DLL; see
  [Project Assembly](../design/project/project-assembly.md).
- **`camera.rs`**: `EditorCamera` (orbit and fly) with its own `ActionMap`.
  The scene view never reads the project map, and the Game tab reads only the
  project map.
- **`selection.rs`**: the selection (game objects or an asset).
- **`icons.rs`**, **`task_id.rs`**: embedded icons, and the ids and messages
  of background tasks.

## Game projects

A project is a crate whose library builds as a `cdylib` with a `plugin_main`
entry point that registers its types, and as an `rlib` for its standalone
player binary.

- **`sandbox`**: the tank game: `arena`, `tank` and `tank_camera`, `sentry`,
  `spawn`, `combat` and `combat_vfx`, `match_rules` and `game_state`,
  `vision` (fog of war), `brush`, `track_marks`, `terrain` (drive surfaces),
  `collision` (its collision groups) and the `hud`. `sandbox/examples/` holds
  the arena and water scene builders. See
  [Sandbox Gameplay](../design/sandbox/gameplay.md) and
  [Tank Game Visual Direction](../design/art/tank-game-direction.md).
- **`demos`**: demo scenes, with the terrain demo's rover as its only code.

## Seams

The interfaces that hold between modules. Extend through them.

- **`Asset`** (`assets/asset.rs`): a type the registry loads by file
  extension, reloads in place and hands out as `AssetRef<T>`.
- **`ContextSeed`** (`utils/deserialize_context.rs`): a serde
  `DeserializeSeed` that carries a context through deserialization, so asset
  references resolve as a scene or asset loads;
  `#[derive(DeserializeWithContext)]` generates its impls.
- **`ComponentRef<T>` and the hook traits** (`component/component.rs`): a hook
  reads and writes its own component through the scene, with no copy; see
  [Components](../design/scene/components.md#hooks).
- **`ShaderLoader`** (`calyx_shader`): resolves and reads a shader's files for
  the preprocessor; `AssetShaderLoader` reads the asset roots and
  `MemoryLoader` serves tests.
- **`DrawCollection`** (`render/draw_collection.rs`): what one scene draws in
  a frame, collected once and shared by every view of it.
- **`PostProcessPass`** (`render/postprocess/pass.rs`): one full-screen pass
  of the chain: its stage, the scene inputs it reads, whether it runs, and
  how it prepares and records.
- **`BlendEffect`** (`render/postprocess/blend.rs`): how an effect component
  blends field by field when a local volume applies over the settings below,
  and its neutral value to fade in from.
- **`UiBackend`** (`ui/backend.rs`): the paint backend the runtime UI renders
  through.
- **`HostCommand`** (`runtime/host_command.rs`): the one channel from game
  code to its host (today only `Quit`). A hook pushes a command into the
  `HostCommands` resource (`ResourceMap::host_commands_mut`), and
  `GameContext::update_scene` returns the frame's commands, in order and once
  each. The editor's `update_game` and the player match on every command
  without a catch-all, so a new variant has to be handled by each host:
  `Quit` stops play in the editor and closes the player's window.
- **`plugin_main` and `TypeRegistry::register_crate`**: the project boundary.
  A project's exported `plugin_main` calls `register_crate` with its crate
  name, which runs only that crate's `inventory` registrations.
- **`SceneEdit`** (`calyx_editor/src/scene_edit.rs`): the editor's write
  transaction on the authoring scene. It captures the scene at its first
  write, commits everything written as one undo step, and records nothing
  while a simulation copy of the scene exists.
- **`Panel`**, **`AssetOpener`**, **`TypeInspector`** and **`AssetInspector`**
  (`calyx_editor`): the editor's extension points for panels, opening an
  asset type, and inspecting a value or asset type.

## Conventions

- Shared ownership is `Ref<T>`, which carries a UUID.
- Components are Legion components with a reflected, type-erased side; the
  editor's inspectors are generated from `Reflect` and `TypeInfo`.
- Hot reload: the file watcher reloads assets in the `AssetRegistry`, and
  `calyx_shader` dependencies decide which shaders an included file reloads.
- Scenes are `.cxscene` files, JSON-serialized `SceneData`.

## Build notes

- Windows links with `rust-lld.exe` and `-Cprefer-dynamic` for fast
  iteration (`.cargo/config.toml`); Linux passes
  `-Clink-arg=-Wl,--undefined-version` for shared library compatibility.
- The pinned nightly and the sccache workaround are in
  [Toolchain](../process/toolchain.md).
- The project DLL links its own engine copy, so engine features must resolve
  the same in the DLL's build as in the editor's; the pins, the statics that
  exist twice and the reload sequence are in
  [Project Assembly](../design/project/project-assembly.md).
- `[profile.dev.build-override]` keeps full debug info for build scripts and
  proc macros, so both builds compile engine's proc macros alike.
- The `release-with-debug` profile is for profiling.
- Under WSL, a test or editor process that created a wgpu instance can die
  with SIGSEGV as it exits, whether or not it loaded a project DLL. The crash
  is on a thread of NVIDIA's WSL D3D12 driver (`libnvwgf2umx.so`), which
  Mesa's default Gallium driver there, `d3d12`, runs on for the GLES backend.
  `GALLIUM_DRIVER=llvmpipe` avoids it. Native Linux and CI are not affected.
