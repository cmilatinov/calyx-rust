use crate::assets::AssetRegistry;
use crate::class_registry::ComponentRegistry;
use crate::component::{Component, ComponentID};
use crate::context::{
    update_scene, AssetContext, GameContext, ReadOnlyRegistryContext, RegistryContext,
};
use crate::core::Ref;
use crate::input::Input;
use crate::reflect::type_registry::TypeRegistry;
use crate::render::{ParticleSimulation, RenderContext};
use crate::resource::ResourceMap;
use crate::scene::{GameObject, Scene};
use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use egui_wgpu::wgpu;
use nalgebra_glm::{distance, Mat4, Vec3};
use std::fmt::Debug;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// Fixed viewport size used by headless test input helpers.
const DEFAULT_TEST_SCREEN_SIZE: Vec2 = Vec2::new(1280.0, 720.0);
const MAX_CONNECT_ITERS: usize = 100;

/// Default fixed simulation step used by headless test runners.
pub const FIXED_TEST_STEP_SECONDS: f64 = 1.0 / 60.0;

struct TestGpu {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

/// Features the editor requests that scene and gizmo shaders rely on; enabled
/// here when the test adapter supports them.
fn desired_test_features() -> wgpu::Features {
    wgpu::Features::BUFFER_BINDING_ARRAY
        | wgpu::Features::TEXTURE_BINDING_ARRAY
        | wgpu::Features::STORAGE_RESOURCE_BINDING_ARRAY
        | wgpu::Features::UNIFORM_BUFFER_AND_STORAGE_TEXTURE_ARRAY_NON_UNIFORM_INDEXING
        | wgpu::Features::FLOAT32_FILTERABLE
}

/// Limits matching the editor's device, clamped to what the adapter supports.
fn test_limits(adapter: &wgpu::Adapter) -> wgpu::Limits {
    let supported = adapter.limits();
    wgpu::Limits {
        max_storage_textures_per_shader_stage: 5
            .min(supported.max_storage_textures_per_shader_stage),
        max_uniform_buffers_per_shader_stage: 30
            .min(supported.max_uniform_buffers_per_shader_stage),
        ..Default::default()
    }
}

fn test_gpu() -> &'static TestGpu {
    static GPU: OnceLock<TestGpu> = OnceLock::new();
    GPU.get_or_init(|| {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        let request_adapter = |force_fallback_adapter| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: None,
                force_fallback_adapter,
            }))
        };
        let adapter = request_adapter(false)
            .or_else(|| request_adapter(true))
            .expect("no wgpu adapter found, including fallback adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                required_features: desired_test_features() & adapter.features(),
                required_limits: test_limits(&adapter),
                ..Default::default()
            },
            None,
        ))
        .expect("failed to create wgpu device");
        TestGpu {
            adapter,
            device,
            queue,
        }
    })
}

/// Returns a shared headless render context suitable for tests.
pub fn test_render_context() -> Arc<RenderContext> {
    static RENDER_CONTEXT: OnceLock<Arc<RenderContext>> = OnceLock::new();
    RENDER_CONTEXT
        .get_or_init(|| {
            let gpu = test_gpu();
            Arc::new(RenderContext::headless(
                Arc::new(gpu.device.clone()),
                Arc::new(gpu.queue.clone()),
            ))
        })
        .clone()
}

/// Returns the adapter behind the shared test device, for tests that query
/// what it supports.
pub fn test_adapter() -> &'static wgpu::Adapter {
    &test_gpu().adapter
}

/// Builds a registry context with the default engine registrations.
pub fn test_registries() -> ReadOnlyRegistryContext {
    let render_context = test_render_context();
    let type_registry = Ref::new(TypeRegistry::new());
    let component_registry = Ref::new(ComponentRegistry::new(&type_registry.read()));
    let asset_registry = AssetRegistry::new_test(
        PathBuf::from("."),
        render_context,
        type_registry.clone(),
        component_registry.clone(),
    );
    ReadOnlyRegistryContext {
        assets: asset_registry.readonly(),
        types: type_registry.readonly(),
        components: component_registry.readonly(),
    }
}

/// Temporary copy of the repository asset roots for tests that load assets.
///
/// The asset registry rewrites `.meta` files while scanning, so tests must never point it at the
/// tracked asset directories. The copy is deleted when the value is dropped, so keep it alive for
/// as long as the registry or context built over it is in use.
pub struct TestAssets {
    dir: PathBuf,
    engine_root: Option<PathBuf>,
    sandbox_root: Option<PathBuf>,
    fixtures_root: Option<PathBuf>,
}

impl TestAssets {
    /// Copies the engine asset root (`assets/`).
    pub fn engine() -> Self {
        Self::copy(true, false, false)
    }

    /// Copies the sandbox asset root (`sandbox/assets/`).
    pub fn sandbox() -> Self {
        Self::copy(false, true, false)
    }

    /// Copies the engine and sandbox asset roots, which the sandbox game loads.
    pub fn game() -> Self {
        Self::copy(true, true, false)
    }

    /// Copies the engine and sandbox asset roots and the model fixtures
    /// (`crates/calyx_engine/tests/fixtures/models/`).
    pub fn models() -> Self {
        Self::copy(true, true, true)
    }

    fn copy(engine: bool, sandbox: bool, fixtures: bool) -> Self {
        let dir = std::env::temp_dir().join(format!("calyx-test-assets-{}", uuid::Uuid::new_v4()));
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let copy_root = |source: &str, name: &str| {
            let source = dunce::canonicalize(workspace.join(source))
                .unwrap_or_else(|e| panic!("{source} dir not found: {e}"));
            let target = dir.join(name);
            copy_dir_all(&source, &target)
                .unwrap_or_else(|e| panic!("failed to copy {}: {e}", source.display()));
            target
        };
        let engine_root = engine.then(|| copy_root("assets", "assets"));
        let sandbox_root = sandbox.then(|| copy_root("sandbox/assets", "sandbox_assets"));
        let fixtures_root = fixtures.then(|| {
            copy_root(
                "crates/calyx_engine/tests/fixtures/models",
                "fixture_assets",
            )
        });
        Self {
            dir,
            engine_root,
            sandbox_root,
            fixtures_root,
        }
    }

    /// Copied engine asset root.
    pub fn engine_root(&self) -> &Path {
        self.engine_root
            .as_deref()
            .expect("engine assets not copied")
    }

    /// Copied sandbox asset root.
    pub fn sandbox_root(&self) -> &Path {
        self.sandbox_root
            .as_deref()
            .expect("sandbox assets not copied")
    }

    /// Copied model fixtures root.
    pub fn fixtures_root(&self) -> &Path {
        self.fixtures_root
            .as_deref()
            .expect("model fixtures not copied")
    }

    /// All copied asset roots, suitable for building a registry.
    pub fn roots(&self) -> Vec<PathBuf> {
        [&self.engine_root, &self.sandbox_root, &self.fixtures_root]
            .into_iter()
            .flatten()
            .cloned()
            .collect()
    }

    /// Builds a registry context over the copied roots.
    pub fn registries(&self) -> ReadOnlyRegistryContext {
        test_registries_with_assets(self.roots())
    }

    /// Builds an asset context over the copied roots.
    pub fn asset_context(&self) -> AssetContext {
        test_asset_context_with_assets(self.roots())
    }
}

impl Drop for TestAssets {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn copy_dir_all(source: &Path, target: &Path) -> io::Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let destination = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &destination)?;
        } else {
            fs::copy(entry.path(), destination)?;
        }
    }
    Ok(())
}

/// Builds a registry context backed by the provided asset roots.
pub fn test_registries_with_assets(asset_paths: Vec<PathBuf>) -> ReadOnlyRegistryContext {
    let render_context = test_render_context();
    let type_registry = Ref::new(TypeRegistry::new());
    let component_registry = Ref::new(ComponentRegistry::new(&type_registry.read()));
    let asset_registry = AssetRegistry::new_test_with_assets(
        asset_paths,
        render_context,
        type_registry.clone(),
        component_registry.clone(),
    );
    ReadOnlyRegistryContext {
        assets: asset_registry.readonly(),
        types: type_registry.readonly(),
        components: component_registry.readonly(),
    }
}

/// Builds an asset context backed by the provided asset roots.
pub fn test_asset_context_with_assets(asset_paths: Vec<PathBuf>) -> AssetContext {
    let render_context = test_render_context();
    let type_registry = Ref::new(TypeRegistry::new());
    let component_registry = Ref::new(ComponentRegistry::new(&type_registry.read()));
    let asset_registry = AssetRegistry::new_test_with_assets(
        asset_paths,
        render_context.clone(),
        type_registry.clone(),
        component_registry.clone(),
    );
    AssetContext {
        render_context,
        registries: RegistryContext {
            types: type_registry,
            components: component_registry,
            assets: asset_registry,
        },
    }
}

/// Returns an empty test scene bound to default registries.
pub fn test_scene() -> Scene {
    test_registries().scene()
}

/// Returns a test game context with default resources and registries.
pub fn test_game_context() -> GameContext {
    let render_context = test_render_context();
    let type_registry = Ref::new(TypeRegistry::new());
    let component_registry = Ref::new(ComponentRegistry::new(&type_registry.read()));
    let asset_registry = AssetRegistry::new_test(
        PathBuf::from("."),
        render_context.clone(),
        type_registry.clone(),
        component_registry.clone(),
    );
    let assets = AssetContext {
        render_context,
        registries: RegistryContext {
            types: type_registry,
            components: component_registry,
            assets: asset_registry,
        },
    };
    GameContext::new(assets)
}

/// Builder for small scene setups used in tests.
pub struct SceneBuilder {
    scene: Scene,
}

impl SceneBuilder {
    /// Creates a builder bound to `registries`.
    pub fn new(registries: ReadOnlyRegistryContext) -> Self {
        Self {
            scene: registries.scene(),
        }
    }

    /// Spawns a root-level game object with `name`.
    pub fn spawn(&mut self, name: impl Into<String>) -> GameObject {
        self.spawn_with_parent(None, name)
    }

    /// Spawns a child game object with `name`.
    pub fn spawn_child(&mut self, parent: GameObject, name: impl Into<String>) -> GameObject {
        self.spawn_with_parent(Some(parent), name)
    }

    /// Adds `component` to `game_object`.
    pub fn add_component<T: Component + Send + Sync + 'static>(
        &mut self,
        game_object: GameObject,
        component: T,
    ) -> &mut Self {
        self.scene.add_component(game_object, component);
        self
    }

    /// Sets the local transform matrix for `game_object`.
    pub fn set_transform(&mut self, game_object: GameObject, matrix: impl Into<Mat4>) -> &mut Self {
        self.scene.set_transform(game_object, &matrix.into());
        self
    }

    /// Returns the current scene by shared reference.
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Returns the current scene by mutable reference.
    pub fn scene_mut(&mut self) -> &mut Scene {
        &mut self.scene
    }

    /// Finishes building and returns the scene.
    pub fn finish(self) -> Scene {
        self.scene
    }

    fn spawn_with_parent(
        &mut self,
        parent: Option<GameObject>,
        name: impl Into<String>,
    ) -> GameObject {
        let component_id = ComponentID {
            name: name.into(),
            ..Default::default()
        };
        self.scene.create(Some(component_id), parent)
    }
}

/// Headless scene driver for deterministic update and input tests.
pub struct HeadlessSceneRunner {
    registries: ReadOnlyRegistryContext,
    resources: ResourceMap,
    scene: Scene,
    input_context: egui::Context,
    input_active: bool,
    pending_events: Vec<Event>,
    modifiers: Modifiers,
    screen_rect: Rect,
    step_seconds: f64,
    particles: Option<ParticleSimulation>,
}

impl Default for HeadlessSceneRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl HeadlessSceneRunner {
    /// Creates a runner with a fresh test scene.
    pub fn new() -> Self {
        Self::from_scene(test_scene())
    }

    /// Creates a runner for an existing scene. Input follows the project action
    /// map (`input.cxinput`) of the scene's asset registry, as the player does;
    /// without one no actions are bound. Panics when that map fails to load.
    pub fn from_scene(mut scene: Scene) -> Self {
        scene.start();
        let registries = scene.registries().clone();
        let mut resources = ResourceMap::new();
        resources
            .input_mut()
            .load_project_action_map(&registries.assets.read())
            .unwrap_or_else(|error| panic!("the project action map should load: {error}"));
        Self {
            registries,
            resources,
            scene,
            input_context: egui::Context::default(),
            input_active: true,
            pending_events: Vec::new(),
            modifiers: Modifiers::default(),
            screen_rect: Rect::from_min_size(Pos2::ZERO, DEFAULT_TEST_SCREEN_SIZE),
            step_seconds: FIXED_TEST_STEP_SECONDS,
            particles: None,
        }
    }

    /// Simulates the scene's particles on the test GPU after each step, as
    /// the editor and player do.
    pub fn with_particles(mut self) -> Self {
        self.particles = Some(ParticleSimulation::new());
        self
    }

    /// Returns the particle simulation, when [`Self::with_particles`] added
    /// one.
    pub fn particles(&self) -> Option<&ParticleSimulation> {
        self.particles.as_ref()
    }

    /// Replaces the currently loaded scene.
    pub fn load_scene(&mut self, mut scene: Scene) {
        scene.start();
        self.registries = scene.registries().clone();
        self.scene = scene;
    }

    /// Returns the active scene.
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Returns the active scene mutably.
    pub fn scene_mut(&mut self) -> &mut Scene {
        &mut self.scene
    }

    /// Returns the runner resource map.
    pub fn resources(&self) -> &ResourceMap {
        &self.resources
    }

    /// Returns the runner resource map mutably.
    pub fn resources_mut(&mut self) -> &mut ResourceMap {
        &mut self.resources
    }

    /// Enables or disables input processing.
    pub fn set_input_active(&mut self, active: bool) {
        self.input_active = active;
    }

    /// Overrides the fixed step duration in seconds.
    pub fn set_step_seconds(&mut self, seconds: f64) {
        self.step_seconds = seconds;
    }

    /// Sets active keyboard modifiers for queued events.
    pub fn set_modifiers(&mut self, modifiers: Modifiers) {
        self.modifiers = modifiers;
    }

    /// Queues a raw egui input event for the next step.
    pub fn enqueue_event(&mut self, event: Event) {
        self.pending_events.push(event);
    }

    /// Queues a key press.
    pub fn press_key(&mut self, key: Key) {
        self.enqueue_event(Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: self.modifiers,
        });
    }

    /// Queues a key release.
    pub fn release_key(&mut self, key: Key) {
        self.enqueue_event(Event::Key {
            key,
            physical_key: Some(key),
            pressed: false,
            repeat: false,
            modifiers: self.modifiers,
        });
    }

    /// Queues a pointer move.
    pub fn move_pointer(&mut self, position: Pos2) {
        self.enqueue_event(Event::PointerMoved(position));
    }

    /// Queues a pointer button press.
    pub fn press_pointer_button(&mut self, position: Pos2, button: PointerButton) {
        self.enqueue_event(Event::PointerButton {
            pos: position,
            button,
            pressed: true,
            modifiers: self.modifiers,
        });
    }

    /// Queues a pointer button release.
    pub fn release_pointer_button(&mut self, position: Pos2, button: PointerButton) {
        self.enqueue_event(Event::PointerButton {
            pos: position,
            button,
            pressed: false,
            modifiers: self.modifiers,
        });
    }

    /// Runs scene preparation only.
    pub fn prepare(&mut self) {
        self.scene.prepare();
    }

    /// Advances the scene by one fixed step.
    pub fn step(&mut self) {
        self.prepare();
        self.resources.time_mut().advance_by(self.step_seconds);

        let raw_input = RawInput {
            screen_rect: Some(self.screen_rect),
            time: Some(self.resources.time().static_time()),
            predicted_dt: self.step_seconds as f32,
            modifiers: self.modifiers,
            events: std::mem::take(&mut self.pending_events),
            ..Default::default()
        };
        self.input_context.begin_pass(raw_input);

        // Bind the whole screen as the input surface, as the editor's game
        // view does, so scripts that read the surface rect (cursor aiming)
        // behave as they do in play.
        let surface = egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(&self.input_context, |ui| {
                ui.allocate_rect(ui.max_rect(), egui::Sense::hover())
            })
            .inner;
        let input = Input::from_ctx(
            &self.input_context,
            Some(&surface),
            self.resources.input_mut(),
            self.input_active,
        );
        match &mut self.particles {
            Some(particles) => update_scene(
                &mut self.scene,
                &self.registries,
                &mut self.resources,
                &input,
                particles,
                &test_render_context(),
            ),
            None => self
                .scene
                .update(&self.registries, &mut self.resources, &input),
        }
        let _ = self.input_context.end_pass();
    }

    /// Advances the scene by `step_count` fixed steps.
    pub fn step_many(&mut self, step_count: usize) {
        for _ in 0..step_count {
            self.step();
        }
    }
}

/// Convenience harness that spins up one host and multiple network clients.
pub struct NetworkTestHarness {
    /// Host followed by all client contexts.
    pub contexts: Vec<GameContext>,
}

impl NetworkTestHarness {
    /// Creates a host and `client_count` connected client contexts.
    pub fn new(client_count: usize) -> Self {
        let mut host = test_game_context();
        host.resources
            .network_mut()
            .host("127.0.0.1:0".parse().unwrap())
            .expect("failed to host");

        let server_addr = host
            .resources
            .network()
            .server
            .as_ref()
            .unwrap()
            .bound_addr();

        let mut contexts = vec![host];
        for _ in 0..client_count {
            let mut ctx = test_game_context();
            ctx.resources
                .network_mut()
                .client
                .connect(server_addr)
                .expect("failed to connect");
            contexts.push(ctx);
        }

        Self { contexts }
    }

    /// Connects one more client to the host, as a peer joining mid-session,
    /// and returns its client index. It is connected once
    /// [`NetworkTestHarness::wait_connected`] returns.
    pub fn add_client(&mut self) -> usize {
        let server_addr = self
            .host()
            .resources
            .network()
            .server
            .as_ref()
            .unwrap()
            .bound_addr();
        let mut ctx = test_game_context();
        ctx.resources
            .network_mut()
            .client
            .connect(server_addr)
            .expect("failed to connect");
        self.contexts.push(ctx);
        self.client_count() - 1
    }

    /// Returns the host context.
    pub fn host(&self) -> &GameContext {
        &self.contexts[0]
    }

    /// Returns the host context mutably.
    pub fn host_mut(&mut self) -> &mut GameContext {
        &mut self.contexts[0]
    }

    /// Returns the client context at `index`.
    pub fn client(&self, index: usize) -> &GameContext {
        &self.contexts[index + 1]
    }

    /// Returns the client context at `index` mutably.
    pub fn client_mut(&mut self, index: usize) -> &mut GameContext {
        &mut self.contexts[index + 1]
    }

    /// Advances networking for every context once.
    pub fn pump(&mut self) {
        for ctx in &mut self.contexts {
            ctx.update();
        }
    }

    /// Pumps networking until all clients report connected or a timeout occurs.
    pub fn wait_connected(&mut self) -> bool {
        for _ in 0..MAX_CONNECT_ITERS {
            self.pump();
            let all_connected = self.contexts[1..]
                .iter()
                .all(|ctx| ctx.resources.network().client.is_connected());
            if all_connected {
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        false
    }

    /// Returns the number of client contexts.
    pub fn client_count(&self) -> usize {
        self.contexts.len() - 1
    }
}

/// Asserts that `game_object` still exists in `scene`.
pub fn assert_entity_exists(scene: &Scene, game_object: GameObject) {
    assert!(
        scene.entry(game_object).is_some(),
        "expected game object `{}` to exist",
        scene.name(game_object)
    );
}

/// Asserts that `game_object` is within `tolerance` of `expected`.
pub fn assert_position_near(
    scene: &Scene,
    game_object: GameObject,
    expected: Vec3,
    tolerance: f32,
) {
    let actual = scene.world_transform(game_object).position;
    let delta = distance(&actual, &expected);
    assert!(
        delta <= tolerance,
        "expected `{}` at {:?} +/- {}, got {:?}",
        scene.name(game_object),
        expected,
        tolerance,
        actual
    );
}

/// Asserts that a component-derived value equals `expected`.
pub fn assert_component_value<T, V, F>(
    scene: &Scene,
    game_object: GameObject,
    reader: F,
    expected: V,
) where
    T: Component,
    V: Debug + PartialEq,
    F: FnOnce(&T) -> V,
{
    let actual = scene
        .read_component::<T, _, _>(game_object, reader)
        .unwrap_or_else(|| {
            panic!(
                "expected component `{}` on `{}`",
                std::any::type_name::<T>(),
                scene.name(game_object)
            )
        });
    assert_eq!(
        actual,
        expected,
        "unexpected `{}` component value on `{}`",
        std::any::type_name::<T>(),
        scene.name(game_object)
    );
}

#[cfg(test)]
mod tests {
    use super::{
        test_asset_context_with_assets, test_registries, HeadlessSceneRunner, SceneBuilder,
        TestAssets,
    };
    use crate as calyx_engine;
    use crate::component::{
        Component, ComponentEventContext, ComponentUpdate, ReflectComponent, ReflectComponentUpdate,
    };
    use crate::context::GameContext;
    use crate::input::{ActionMap, InputBinding, PROJECT_ACTION_MAP_NAME};
    use crate::input::{Input, InputState};
    use crate::reflect::{Reflect, ReflectDefault};
    use crate::resource::{Resource, ResourceMap};
    use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
    use egui::{Key, PointerButton, Pos2, RawInput, Vec2};
    use serde::{Deserialize, Serialize};

    #[derive(Default, Resource, TypeUuid)]
    #[uuid = "c5fa8823-a151-4543-8559-5307172cd255"]
    struct InputProbe {
        shoot_presses: usize,
        forward_axis: f32,
        cursor_delta: Vec2,
    }

    #[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
    #[uuid = "8336b78b-5e57-474a-95ce-904bd42b5135"]
    #[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
    struct TestInputComponent;

    impl Component for TestInputComponent {}

    impl ComponentUpdate for TestInputComponent {
        fn update(&self, _ctx: ComponentEventContext, resources: &mut ResourceMap, input: &Input) {
            // The registry runs every update hook for every object, so a
            // scene without the probe resource is another test's scene.
            let Some(probe) = resources.resource_mut::<InputProbe>() else {
                return;
            };
            if input.action("shoot").just_pressed() {
                probe.shoot_presses += 1;
            }
            probe.forward_axis = input.axis("move_forward");
            probe.cursor_delta = input.cursor_delta();
        }
    }

    #[test]
    fn headless_runner_applies_queued_input_events() {
        let mut builder = SceneBuilder::new(test_registries());
        let game_object = builder.spawn("input-probe");
        builder.add_component(game_object, TestInputComponent);

        let mut runner = HeadlessSceneRunner::from_scene(builder.finish());
        runner.resources_mut().insert(InputProbe::default());
        let mut map = ActionMap::new();
        map.bind_action("shoot", InputBinding::Key(Key::Space));
        map.bind_axis(
            "move_forward",
            InputBinding::Key(Key::W),
            InputBinding::Key(Key::S),
        );
        runner.resources_mut().input_mut().set_action_map(map);

        runner.press_key(Key::W);
        runner.press_key(Key::Space);
        runner.step();

        let probe = runner.resources().resource::<InputProbe>().unwrap();
        assert_eq!(probe.shoot_presses, 1);
        assert_eq!(probe.forward_axis, 1.0);

        runner.release_key(Key::Space);
        runner.release_key(Key::W);
        runner.step();

        let probe = runner.resources().resource::<InputProbe>().unwrap();
        assert_eq!(probe.forward_axis, 0.0);
    }

    /// Lists every file under `root` with its size and modification time.
    fn snapshot(root: &std::path::Path) -> Vec<(std::path::PathBuf, u64, std::time::SystemTime)> {
        let mut files = Vec::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let entry = entry.unwrap();
                let metadata = entry.metadata().unwrap();
                if metadata.is_dir() {
                    pending.push(entry.path());
                } else {
                    files.push((entry.path(), metadata.len(), metadata.modified().unwrap()));
                }
            }
        }
        files.sort();
        files
    }

    #[test]
    fn loading_test_assets_does_not_modify_source_asset_directory() {
        let source = dunce::canonicalize(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"),
        )
        .unwrap();
        let before = snapshot(&source);

        let assets = TestAssets::engine();
        let registries = assets.registries();
        assert!(registries.assets.read().asset_id("meshes/cube").is_some());
        drop(registries);

        assert_eq!(before, snapshot(&source));
        let copy = assets.engine_root().to_path_buf();
        assert!(copy.starts_with(std::env::temp_dir()));
        drop(assets);
        assert!(!copy.exists(), "temporary copy should be removed on drop");
    }

    fn input_probe_runner() -> HeadlessSceneRunner {
        let mut builder = SceneBuilder::new(test_registries());
        let game_object = builder.spawn("input-probe");
        builder.add_component(game_object, TestInputComponent);
        let mut runner = HeadlessSceneRunner::from_scene(builder.finish());
        runner.resources_mut().insert(InputProbe::default());
        runner
    }

    fn shoot_presses(runner: &HeadlessSceneRunner) -> usize {
        runner
            .resources()
            .resource::<InputProbe>()
            .unwrap()
            .shoot_presses
    }

    #[test]
    fn rebound_action_fires_on_the_new_binding() {
        let mut runner = input_probe_runner();
        let mut map = ActionMap::new();
        map.bind_action("shoot", InputBinding::Key(Key::Enter));
        runner.resources_mut().input_mut().set_action_map(map);

        runner.press_key(Key::Space);
        runner.step();
        assert_eq!(shoot_presses(&runner), 0);

        runner.release_key(Key::Space);
        runner.press_key(Key::Enter);
        runner.step();
        assert_eq!(shoot_presses(&runner), 1);
    }

    #[test]
    fn project_action_map_asset_drives_the_scene() {
        let dir = std::env::temp_dir().join(format!("calyx-input-asset-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut map = ActionMap::new();
        map.bind_action(
            "shoot",
            InputBinding::PointerButton(PointerButton::Secondary),
        );
        let asset_path = dir.join(format!("{PROJECT_ACTION_MAP_NAME}.cxinput"));
        serde_json::to_writer_pretty(std::fs::File::create(&asset_path).unwrap(), &map).unwrap();

        let assets = test_asset_context_with_assets(vec![dir.clone()]);
        let game = GameContext::new(assets.clone());
        let mut builder = SceneBuilder::new(assets.registries.lock_read());
        let game_object = builder.spawn("input-probe");
        builder.add_component(game_object, TestInputComponent);
        let mut runner = HeadlessSceneRunner::from_scene(builder.finish());
        runner.resources_mut().insert(InputProbe::default());
        runner
            .resources_mut()
            .insert(game.resources.input().clone());

        runner.press_key(Key::Space);
        runner.step();
        assert_eq!(shoot_presses(&runner), 0);

        runner.release_key(Key::Space);
        runner.press_pointer_button(Pos2::new(10.0, 10.0), PointerButton::Secondary);
        runner.step();
        assert_eq!(shoot_presses(&runner), 1);

        // A hot reload updates the asset handle in place; the running input follows it.
        let mut reloaded = ActionMap::new();
        reloaded.bind_action("shoot", InputBinding::Key(Key::Enter));
        let handle = assets
            .registries
            .assets
            .read()
            .load::<ActionMap>(PROJECT_ACTION_MAP_NAME)
            .unwrap();
        *handle.write() = reloaded;
        runner.release_pointer_button(Pos2::new(10.0, 10.0), PointerButton::Secondary);
        runner.press_pointer_button(Pos2::new(10.0, 10.0), PointerButton::Secondary);
        runner.step();
        assert_eq!(shoot_presses(&runner), 1);
        runner.press_key(Key::Enter);
        runner.step();
        assert_eq!(shoot_presses(&runner), 2);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cursor_delta_reports_movement_since_the_previous_frame() {
        let mut runner = input_probe_runner();
        let delta = |runner: &HeadlessSceneRunner| {
            runner
                .resources()
                .resource::<InputProbe>()
                .unwrap()
                .cursor_delta
        };

        runner.move_pointer(Pos2::new(10.0, 10.0));
        runner.step();
        assert_eq!(delta(&runner), Vec2::ZERO);

        runner.move_pointer(Pos2::new(25.0, 12.0));
        runner.step();
        assert_eq!(delta(&runner), Vec2::new(15.0, 2.0));

        runner.step();
        assert_eq!(delta(&runner), Vec2::ZERO);

        runner.move_pointer(Pos2::new(35.0, 12.0));
        runner.step();
        assert_eq!(delta(&runner), Vec2::new(10.0, 0.0));
    }

    #[test]
    fn input_surfaces_in_one_pass_share_the_cursor_delta() {
        let ctx = egui::Context::default();
        let mut state = InputState::default();
        let pass = |position: Pos2| RawInput {
            events: vec![egui::Event::PointerMoved(position)],
            ..Default::default()
        };

        ctx.begin_pass(pass(Pos2::new(10.0, 10.0)));
        Input::from_ctx(&ctx, None, &mut state, true);
        let _ = ctx.end_pass();

        ctx.begin_pass(pass(Pos2::new(25.0, 12.0)));
        let first = Input::from_ctx(&ctx, None, &mut state, true);
        let second = Input::from_ctx(&ctx, None, &mut state, true);
        let inactive = Input::from_ctx(&ctx, None, &mut state, false);
        let _ = ctx.end_pass();

        assert_eq!(first.cursor_delta(), Vec2::new(15.0, 2.0));
        assert_eq!(second.cursor_delta(), Vec2::new(15.0, 2.0));
        assert_eq!(inactive.cursor_delta(), Vec2::ZERO);
    }

    #[test]
    fn malformed_project_action_map_is_reported() {
        let dir = std::env::temp_dir().join(format!("calyx-input-bad-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{PROJECT_ACTION_MAP_NAME}.cxinput")),
            "not json",
        )
        .unwrap();
        let assets = test_asset_context_with_assets(vec![dir.clone()]);

        let mut state = InputState::default();
        let result = state.load_project_action_map(&assets.registries.assets.read());
        assert!(result.is_err());

        let empty = std::env::temp_dir().join(format!("calyx-input-none-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&empty).unwrap();
        let assets = test_asset_context_with_assets(vec![empty.clone()]);
        let result = state.load_project_action_map(&assets.registries.assets.read());
        assert!(result.is_ok());

        std::fs::remove_dir_all(dir).unwrap();
        std::fs::remove_dir_all(empty).unwrap();
    }
}
