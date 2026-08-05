---
inclusion: auto
---

# Calyx Engine — Project Overview

Calyx is a 3D game engine and editor written in Rust, targeting a multiplayer tank game. The workspace has five crates:

## Workspace Crates

### `engine` (library)
Core runtime. Modules:
- `assets/` — Asset trait, AssetRegistry (hot-reload via notify file watcher), AssetRef<T> for lazy references. Types: Mesh (russimp-ng OBJ import), Texture (PNG/JPG/WebP via image crate), Shader (WGSL + naga introspection), Material (shader variable binding), Animation, AnimationGraph, Prefab, Skybox.
- `component/` — Legion ECS components. `Component` trait for destroy/draw_gizmos. `ComponentStart`, `ComponentUpdate`, and `ComponentReset` are separate reflected lifecycle traits (avoids aliased `&mut Scene`). `ComponentInstance` provides type-erased serialization. Built-in: Transform, Camera, Mesh, Collider, RigidBody, DirectionalLight, PointLight, Animator, Bone, NetworkObject.
- `scene/` — Scene (delegates to SceneGraph for hierarchy, GameObjectStore for entity/UUID mapping, TransformCache for world transforms; holds World + PhysicsContext). SceneManager for load/save/simulation. Prefab instantiation with UUID remapping. SceneData for serialized representation.
- `render/` — wgpu-based. RenderContext (eframe or headless), SceneRenderer (mesh/skybox/grid/gizmos), ShaderPreprocessor (#include support), PBR pipeline, mip generators, Camera/Lights uniform management.
- `physics/` — Rapier3D integration. PhysicsContext with fixed timestep (1/60s). Rigid bodies, colliders, query pipeline. Currently passes `&()` for collision event handlers (no events captured).
- `net/` — Renet networking. Client/Server architecture, NetworkObjectId ownership, GameMessage enum, MessageHandler trait, tick-rate accumulator. Sync module for transform interpolation.
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
- `release-with-debug` profile for profiling.

## Asset File Types
- `.wgsl` — Shaders
- `.png`, `.jpg`, `.jpeg`, `.webp` — Textures
- `.obj` — Meshes
- `.cxscene` — Scenes
- `.cxanim` — Animations
- `.meta` — Asset metadata (UUID, name, type)
- `project.toml` — Project configuration
