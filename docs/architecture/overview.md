# Calyx Engine — Project Overview

Calyx is a 3D game engine and editor written in Rust, targeting a multiplayer tank game. The workspace has five crates:

## Workspace Crates

### `engine` (library)
Core runtime. Modules:
- `assets/` — Asset trait, AssetRegistry (hot-reload via notify file watcher), AssetRef<T> for lazy references. Model files import through `assets/import`, which turns a file into a library-independent imported scene (meshes, node tree, baked animations) using ufbx for FBX and OBJ and the gltf crate for glTF and GLB (materials, textures and morph targets are not imported); Collada (`.dae`) is not supported. Types: Mesh (OBJ), Texture (PNG/JPG/WebP via image crate), Shader (WGSL + naga introspection), Material (shader variable binding), Animation, AnimationGraph, Prefab (`.cxprefab`, or an FBX, glTF or GLB model imported into mesh and animation sub-assets), Skybox. Import snapshot tests compare each model against references captured from the former assimp importer, and the GLB fixture's animation against the FBX fixture's; see `engine/tests/fixtures/`.
- `component/` — Legion ECS components. `Component` trait for destroy/draw_gizmos. `ComponentStart`, `ComponentUpdate` and `ComponentReset` as separate reflect_traits for start/update/reset lifecycle (avoids aliased `&mut Scene`). `ComponentInstance` for type-erased serialization. Built-in: Transform, Camera, Mesh, Collider, RigidBody, DirectionalLight, PointLight, Animator, Bone, NetworkObject, ParticleSystem. ParticleSystem (`component/particle/`) holds authoring data only and is built from modules: emission says how many particles spawn, init modules (shape, velocity, forces, lifetime, size) fill each new particle on the CPU, the update module (forces) runs in a compute kernel over a flat `#[repr(C)]` particle buffer, and size and color modules evaluate each particle by normalized age for rendering, also on the GPU. A non-serialized reset counter on the component asks the simulation to re-seed the system.
- `scene/` — Scene (has a `SceneId`, new for every constructed scene including a restored snapshot, so the play copy and the scene it was copied from differ even though their object ids match; delegates to SceneGraph for hierarchy, GameObjectStore for entity/UUID mapping, TransformCache for world transforms; holds World + PhysicsContext). SceneManager for load/save/simulation. Prefab instantiation with UUID remapping. SceneData for serialized representation.
- `render/` — wgpu-based. RenderContext (eframe or headless), SceneRenderer (mesh/skybox/grid/gizmos/particles), ShaderPreprocessor (#include support), PBR pipeline, mip generators, Camera/Lights uniform management. `Shader::compute_pipeline(entry_point)` builds and caches one compute pipeline per entry point, laid out with only the bindings that entry point uses.
  - Particles: `ParticleSimulation` lives on `GameContext` and simulates the particle systems of one scene on the GPU, once per frame, from `context::update_scene`, which runs `Scene::update` and then the simulation; the editor calls it from `update_game` while play runs and the player each frame. It binds to one `SceneId` and releases all its GPU state when it sees another, and drops a system on the first frame its object no longer has a `ComponentParticleSystem`. The CPU keeps per system only emission state, the random generator (seeded from the object's id) and a life ledger of each slot's age and lifetime, so it knows the live count, the `max_particles` cap and the lowest free slot without reading particles back. Kernels (`assets/shaders/particles_simulate.wgsl`): update (`advance_age` and semi-implicit Euler over the frame's queued steps) and spawn scatter. Each `SceneRenderer`'s `ParticleRenderer` keeps only per-view state, draws only when the simulation's `SceneId` is the drawn scene's, and on the GPU evaluates, bitonic-sorts back to front (dead slots last, draw count = live count) and gathers each system into its instance buffer. Thumbnail scenes and the edit scene are never simulated, so they draw no particles.
- `physics/` — Rapier3D integration. PhysicsContext with fixed timestep (1/60s). Rigid bodies, colliders, query pipeline. Currently passes `&()` for collision event handlers (no events captured).
- `net/` — Renet networking. Client/Server architecture, NetworkObjectId ownership, GameMessage enum, MessageHandler trait, tick-rate accumulator. Sync module for transform interpolation. The server is the only relay: clients send to the host, which applies spawns, destroys, ownership transfers and component syncs as a peer. It forwards spawns, destroys and syncs to every other client, and an accepted transfer to every client, its sender included. A transfer, destroy or sync from a client that does not own the object is dropped. Network object ids are `range << 32 | counter`: the server reserves a range for each peer as it joins (the host takes the first, range 0 stays with objects authored into a scene, and a range is never handed out twice in a session) and sends it in `SelfConnected`; each peer's `NetworkIdAllocator` counts up within its own range, so honest peers never collide. The server refuses a client's whole `SpawnPrefab` when an id is outside the sender's range, repeats within the spawn or already exists in the scene (checked before it instantiates anything), or when the ids do not name exactly the prefab's network objects (its instance is then undone); it relays nothing it refused and logs the sender and the offending ids. A client refuses a relayed spawn whose ids do not name exactly the prefab's network objects too, which catches spawns of a prefab the host could not load.
- `reflect/` — Runtime reflection. Reflect trait with field access, TypeRegistry, TypeInfo (struct-only currently). Attribute system for editor metadata.
- `core/` — Ref<T> (Arc<RwLock<T>> + UUID), ReadOnlyRef<T>, WeakRef<T>, Time.
- `resource.rs` — ResourceMap (TypeId → Box<dyn Resource> HashMap). Holds Time, Background, Network.
- `context.rs` — RegistryContext, AssetContext, GameContext. Layered read-only wrappers.
- `class_registry.rs` — ComponentRegistry for type-erased component construction from reflection.
- `math/` — Transform (position/rotation/scale), matrix decomposition, inverse.
- `input/` — Thin wrapper over egui::InputState.

### `editor` (binary)
egui + eframe editor application.
- `panel/` — Modular panels: Viewport, Inspector, SceneHierarchy, ContentBrowser, Terminal, Game, Animator. Panel trait with name/icon/ui.
- `inspector/` — InspectorRegistry with type-specific inspectors (bool, float, int, string, color, transform, vec, asset ref, game object ref, collider, rigid body, UUID). Asset inspectors for Material, Shader, Prefab, AnimationGraph.
- `widgets/` — Tab system, file button, list widget.
- `camera.rs` — EditorCamera (orbit/fly).
- `selection.rs` — Selection state (GameObject or Asset).
- `scene_document.rs` — SceneDocument: the authoring scene's undo history, dirty state, recovery autosave and file. Panels write the scene through a `SceneEdit` (`scene_edit.rs`), which captures the scene at its first write, commits everything written as one undo step, and records nothing while a simulation copy of the scene exists.
- `project_manager.rs` — Project loading, assembly building, background tasks.

### `engine_derive` (proc-macro)
Derive macros:
- `#[derive(Component)]` — Generates ComponentInstance impl (Legion type bridging, serialize/deserialize).
- `#[derive(Reflect)]` — Generates runtime reflection with field info, attributes, TypeName. Requires `#[repr(C)]`.
- `#[derive(TypeUuid)]` — Static UUID from `#[uuid = "..."]` attribute.
- `#[derive(Resource)]` — Marker impl for ResourceMap storage.
- `#[derive(DeserializeWithContext)]` — Context-aware deserialization for asset references.
- `#[reflect_trait]` — Generates ReflectTrait wrapper for trait object registration.
- `impl_reflect_value!` — Reflect impl for primitive/foreign types.
- `fq.rs` — Fully-qualified path helpers (14 unit structs, legacy pattern).

### `project` (library)
Project scaffolding. Generates project.toml, Cargo.toml (from template), and assets/lib.rs. Loads/validates project structure.

### `sandbox` (cdylib + binary)
Example game project. Compiles as dynamic library (plugin) with `plugin_main` entry point for type registration. Contains:
- `player.rs` — ComponentPlayerController (WASD movement, camera-relative, network ownership check).
- `network.rs` — Game-specific network handlers.
- `game.rs` — Standalone game binary.

## Key Patterns
- `Ref<T>` everywhere for shared ownership with UUID tracking.
- Legion ECS with trait-object components (not archetypal queries for game logic).
- Reflection-driven editor: components auto-generate inspector UI via Reflect + TypeInfo.
- Hot-reload: file watcher → AssetRegistry reload. Shader preprocessor handles #include.
- Networking: Renet client/server, fixed tick rate, message queue with typed handlers.
- Scene files: `.cxscene` (JSON serialized SceneData).

## Build Notes
- Windows: uses `rust-lld.exe` linker, `-Cprefer-dynamic` for fast iteration.
- Linux: `-Clink-arg=-Wl,--undefined-version` for shared lib compat.
- The project DLL statically links its own copy of `engine`, and engine types cross the DLL boundary keyed by `TypeId`. The DLL is built in a separate `cargo build --lib`, so engine's dependencies must resolve to the same features there as in the editor build, or the DLL gets another engine build with other `TypeId`s. `engine/engine_feature_pins` pins the editor's features; `editor/tests/project_assembly_features.rs` fails when they drift, and the editor logs an error when it loads an assembly built against another engine.
- The DLL's own copy of `engine` and its dependencies also means its own copy of every `static` and `thread_local!` in them. Code that reaches one from the DLL sees the DLL's copy, not the editor's. The ones that matter:
  - Legion's entity counter (`NEXT_ENTITY`): the DLL's starts over and would hand out entities the editor already did, so a `Scene` keeps a pointer to the allocator of the engine build that created it and spawns every object, from either side, with it.
  - The prefab id remap (`RemapScope`, a thread-local read by `GameObjectRef` deserialization): each component type enters it in its own code through `ComponentInstance::deserialize_remapped`, so a project component sees the remap the editor's prefab instantiation asked for.
  - The `log` logger: `TypeRegistry::register_crate` installs the logger the editor recorded in `begin_plugin_load`, so DLL log records reach the editor's log.
  - Safe as they are: the scene id counter (`SceneId`; the DLL's copy starts over, but the only scenes the particle simulation binds, the editor's play copy and the player's scene, are built by the editor or player's own engine build); Legion's `ID_CLONE_MAPPINGS` (only `World::clone_from`, which the engine never calls), `COMPONENT_VERSION` (only Legion's `maybe_changed` filter, unused; the scene's change set tracks writes) and `WORLD_ID_COUNTER` (keys Legion query caches, and the engine builds a fresh query per use); `warn_once` keys (a warning can show once per copy); `inventory` registrations (each copy collects its own, which is how `register_crate` finds only the DLL's types).
- Reloading the project DLL after a rebuild unloads the previous one, and everything its code backs must go first: Legion storages of project components (their drop, clone and serialize code is the DLL's), the `allocate_entity` pointer of a scene the DLL created, the DLL's type registrations and the component binders made from them. So `ProjectManager` loads the new DLL, then, while the old one is still loaded, has `SceneManager::suspend_scenes` snapshot and empty every scene it reaches, calls `TypeRegistry::unload_plugins` (which resets the registry to the editor's own types, dropping types the new build removed too) and rebuilds the component registry from it. It then registers the new DLL's types, unloads the old DLL and rebuilds each scene from its snapshot with `SceneManager::resume_scenes`. A reload:
  - keeps the authoring scene with its unsaved edits, its file and dirty state, the selection, and the undo and redo history, which are all serialized;
  - keeps each cached scene asset and the scene manager's default scene, even one the cache has evicted, rebuilt in place behind the same reference; prefabs are serialized data and stay as they are;
  - stops a running or paused simulation and drops its scene, as Stop does; Play starts a fresh one;
  - commits pending inspector edits before the reload and takes the inspector's baselines again after it;
  - drops components whose type the new build removed, or that no longer deserialize, from the rebuilt scenes, so saving afterwards writes the scene without them;
  - does not cover resources or asset types project code adds to the editor; the sandbox adds none.
- The editor loads the project DLL with `libloading`, which hands `dlopen` and `dlsym` NUL-terminated strings. `sharedlib`, used before, passed unterminated ones on Linux, so no assembly ever loaded there. Every load is of a fresh shadow copy under its own folder, so on Linux `dlopen` returns a new handle for each build, and dropping the previous one `dlclose`s and unmaps it: after a reload only the new copy is mapped. Linux runs `editor`'s assembly reload test like Windows does.
- Under WSL, a test or editor process that created a wgpu instance can die with SIGSEGV as it exits, whether or not it loaded a project DLL. The crash is on a thread of NVIDIA's WSL D3D12 driver (`libnvwgf2umx.so`), which Mesa's default Gallium driver there, `d3d12`, runs on for the GLES backend. `GALLIUM_DRIVER=llvmpipe` avoids it. Native Linux and CI are not affected.
- `release-with-debug` profile for profiling.

## Asset File Types
- `.wgsl` — Shaders
- `.png`, `.jpg`, `.jpeg`, `.webp` — Textures
- `.obj` — Meshes
- `.fbx`, `.gltf`, `.glb` — Models, imported as prefabs
- `.cxscene` — Scenes
- `.cxanim` — Animations
- `.meta` — Asset metadata (UUID, name, type)
- `project.toml` — Project configuration
