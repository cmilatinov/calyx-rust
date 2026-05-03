use crate::assets::AssetRegistry;
use crate::class_registry::ComponentRegistry;
use crate::component::{Component, ComponentID};
use crate::context::{AssetContext, GameContext, ReadOnlyRegistryContext, RegistryContext};
use crate::core::Ref;
use crate::input::{ActionMap, Input, InputState};
use crate::reflect::type_registry::TypeRegistry;
use crate::render::RenderContext;
use crate::resource::ResourceMap;
use crate::scene::{GameObject, Scene};
use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use egui_wgpu::wgpu;
use nalgebra_glm::{distance, Mat4, Vec3};
use std::fmt::Debug;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// Fixed viewport size used by headless test input helpers.
const DEFAULT_TEST_SCREEN_SIZE: Vec2 = Vec2::new(1280.0, 720.0);
const MAX_CONNECT_ITERS: usize = 100;

/// Default fixed simulation step used by headless test runners.
pub const FIXED_TEST_STEP_SECONDS: f32 = 1.0 / 60.0;

/// Returns a shared headless render context suitable for tests.
pub fn test_render_context() -> Arc<RenderContext> {
    static RENDER_CONTEXT: OnceLock<Arc<RenderContext>> = OnceLock::new();
    RENDER_CONTEXT
        .get_or_init(|| {
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
            let (device, queue) =
                pollster::block_on(adapter.request_device(&Default::default(), None))
                    .expect("failed to create wgpu device");
            Arc::new(RenderContext::headless(Arc::new(device), Arc::new(queue)))
        })
        .clone()
}

fn build_type_registry() -> Ref<TypeRegistry> {
    let mut type_registry = TypeRegistry::new();
    for f in inventory::iter::<crate::ReflectRegistrationFn> {
        (f.function)(&mut type_registry);
    }
    Ref::new(type_registry)
}

/// Builds a registry context with the default engine registrations.
pub fn test_registries() -> ReadOnlyRegistryContext {
    let render_context = test_render_context();
    let type_registry = build_type_registry();
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

/// Builds a registry context backed by the provided asset roots.
pub fn test_registries_with_assets(asset_paths: Vec<PathBuf>) -> ReadOnlyRegistryContext {
    let render_context = test_render_context();
    let type_registry = build_type_registry();
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
    let type_registry = build_type_registry();
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
    let type_registry = build_type_registry();
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
    action_map: ActionMap,
    input_active: bool,
    pending_events: Vec<Event>,
    modifiers: Modifiers,
    screen_rect: Rect,
    step_seconds: f32,
}

impl HeadlessSceneRunner {
    /// Creates a runner with a fresh test scene.
    pub fn new() -> Self {
        Self::from_scene(test_scene())
    }

    /// Creates a runner for an existing scene.
    pub fn from_scene(scene: Scene) -> Self {
        Self {
            registries: scene.registries().clone(),
            resources: ResourceMap::new(),
            scene,
            input_context: egui::Context::default(),
            action_map: ActionMap::default(),
            input_active: true,
            pending_events: Vec::new(),
            modifiers: Modifiers::default(),
            screen_rect: Rect::from_min_size(Pos2::ZERO, DEFAULT_TEST_SCREEN_SIZE),
            step_seconds: FIXED_TEST_STEP_SECONDS,
        }
    }

    /// Replaces the currently loaded scene.
    pub fn load_scene(&mut self, scene: Scene) {
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

    /// Returns the mutable action map used for synthetic input.
    pub fn action_map_mut(&mut self) -> &mut ActionMap {
        &mut self.action_map
    }

    /// Enables or disables input processing.
    pub fn set_input_active(&mut self, active: bool) {
        self.input_active = active;
    }

    /// Overrides the fixed step duration in seconds.
    pub fn set_step_seconds(&mut self, seconds: f32) {
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
            time: Some(self.resources.time().static_time() as f64),
            predicted_dt: self.step_seconds,
            modifiers: self.modifiers,
            events: std::mem::take(&mut self.pending_events),
            ..Default::default()
        };
        self.input_context.begin_pass(raw_input);

        let input = Input::from_ctx(
            &self.input_context,
            None,
            InputState {
                is_active: self.input_active,
                last_cursor_pos: None,
                action_map: self.action_map.clone(),
            },
        );
        self.scene
            .update(&self.registries, &mut self.resources, &input);
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
    use super::{test_registries, HeadlessSceneRunner, SceneBuilder};
    use crate as engine;
    use crate::component::{
        Component, ComponentEventContext, ComponentUpdate, ReflectComponent, ReflectComponentUpdate,
    };
    use crate::input::Input;
    use crate::reflect::{Reflect, ReflectDefault};
    use crate::resource::{Resource, ResourceMap};
    use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
    use egui::Key;
    use serde::{Deserialize, Serialize};

    #[derive(Default, Resource, TypeUuid)]
    #[uuid = "c5fa8823-a151-4543-8559-5307172cd255"]
    #[repr(C)]
    struct InputProbe {
        shoot_presses: usize,
        forward_axis: f32,
    }

    #[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
    #[uuid = "8336b78b-5e57-474a-95ce-904bd42b5135"]
    #[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
    #[repr(C)]
    struct TestInputComponent;

    impl Component for TestInputComponent {}

    impl ComponentUpdate for TestInputComponent {
        fn update(&self, _ctx: ComponentEventContext, resources: &mut ResourceMap, input: &Input) {
            let probe = resources.resource_mut::<InputProbe>().unwrap();
            if input.action("shoot").just_pressed() {
                probe.shoot_presses += 1;
            }
            probe.forward_axis = input.axis("move_forward");
        }
    }

    #[test]
    fn headless_runner_applies_queued_input_events() {
        let mut builder = SceneBuilder::new(test_registries());
        let game_object = builder.spawn("input-probe");
        builder.add_component(game_object, TestInputComponent);

        let mut runner = HeadlessSceneRunner::from_scene(builder.finish());
        runner.resources_mut().insert(InputProbe::default());

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
}
