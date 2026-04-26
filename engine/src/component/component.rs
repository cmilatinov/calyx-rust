use legion::storage::ComponentTypeId;
use legion::world::{Entry, EntryRef};

use engine_derive::reflect_trait;
pub use engine_derive::Component;

use crate as engine;
use crate::context::ReadOnlyRegistryContext;
use crate::input::Input;
use crate::reflect::Reflect;
use crate::render::Gizmos;
use crate::resource::ResourceMap;
use crate::scene::{GameObject, Scene};
use crate::utils::TypeUuidDynamic;

/// Provides type-erased access to a concrete component stored in a Legion entity.
///
/// This trait is automatically implemented by `#[derive(Component)]`. It bridges
/// the gap between Legion's typed storage and the engine's trait-object component
/// model, enabling serialization, reflection, dynamic binding, and editor UI.
///
/// Most component authors should not implement this manually. Derive
/// `Component`, `Reflect`, and `TypeUuid`, then opt into lifecycle traits with
/// reflection attributes:
///
/// ```ignore
/// #[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
/// #[reflect(Component, ComponentUpdate, ComponentReset)]
/// struct PlayerController {
///     speed: f32,
/// }
/// ```
///
/// `ComponentInstance` methods operate on the concrete component stored in the
/// `Scene` ECS world. The component registry keeps prototype instances that use
/// these methods to find, serialize, deserialize, and bind real component values
/// without knowing their Rust type at the call site.
pub trait ComponentInstance: Reflect {
    /// Returns the Legion `ComponentTypeId` for the concrete component type.
    fn component_type_id(&self) -> ComponentTypeId;

    /// Returns a shared reference to this component type on the given entity,
    /// or `None` if the entity does not have it.
    fn get_instance<'a>(&self, entry: &'a EntryRef) -> Option<&'a dyn Component>;

    /// Returns an exclusive reference to this component type on the given entity,
    /// or `None` if the entity does not have it.
    fn get_instance_mut<'a>(&self, entry: &'a mut Entry) -> Option<&'a mut dyn Component>;

    /// Add a new component instance, carried as `Box<dyn Reflect>`, to the entity.
    /// Returns `true` if the downcast to the concrete type succeeded.
    fn bind_instance(&self, entry: &mut Entry, instance: Box<dyn Reflect>) -> bool;

    /// Remove this component type from the entity.
    fn remove_instance(&self, entry: &mut Entry);

    /// Serialize this component's current state to JSON.
    fn serialize(&self) -> Option<serde_json::Value>;

    /// Deserialize a component from JSON, returning an owned `Box<dyn Reflect>`.
    fn deserialize(&self, value: &serde_json::Value) -> Option<Box<dyn Reflect>>;

    /// Convenience: deserialize JSON and assign the result to `self` in place.
    fn deserialize_in_place(&mut self, value: &serde_json::Value) -> bool {
        if let Some(value) = self.deserialize(value) {
            self.assign(value)
        } else {
            false
        }
    }
}

/// Context passed to component lifecycle functions.
///
/// Provides mutable access to the scene and identifies which game object the
/// component belongs to. Registry access is included for asset/type lookups.
///
/// The context is intentionally short-lived. It is safe to query or mutate the
/// scene during the current lifecycle call, but callers should not stash
/// references borrowed from `ctx.scene` beyond the function body. Store stable
/// handles such as `GameObject`, `GameObjectRef`, asset references, or UUIDs
/// instead.
///
/// `registries` are read-only in component callbacks. Use them for component
/// metadata, type information, and asset lookup. Component callbacks should not
/// register new component or asset types at runtime.
pub struct ComponentEventContext<'a> {
    /// Read-only registries for asset, type, and component metadata lookups.
    pub registries: &'a ReadOnlyRegistryContext,
    /// Mutable access to the scene currently invoking the lifecycle hook.
    pub scene: &'a mut Scene,
    /// The game object that owns the component whose hook is being invoked.
    pub game_object: GameObject,
}

/// Marker and infrequent lifecycle hooks for a game component.
///
/// A component's usual lifecycle is:
///
/// 1. A value is constructed from defaults, deserialization, or gameplay code.
/// 2. The value is bound to a `GameObject`.
/// 3. If the type reflects `ComponentReset`, `reset` runs once after binding.
/// 4. Each simulation frame runs `Scene::prepare`, then `Scene::update`, which
///    calls reflected `ComponentUpdate` hooks for active components.
/// 5. When removed, `destroy` runs before the component leaves the object.
///
/// Physics is stepped inside scene preparation/update code before component
/// update hooks observe the frame's simulation state. Components should read
/// collision/contact results from the scene physics context during `update` and
/// write forces or transforms for the next simulation step.
///
/// `draw_gizmos` is editor/debug rendering only. It should visualize state and
/// avoid mutating gameplay data. Runtime gameplay should live in
/// `ComponentUpdate`, not in gizmo drawing.
#[allow(unused)]
#[reflect_trait]
pub trait Component: TypeUuidDynamic + ComponentInstance {
    /// Called when the component is removed from a game object.
    ///
    /// Use this to detach child objects, release runtime-only resources, or
    /// unregister scene-side relationships. Asset handles and ordinary owned
    /// Rust values normally clean themselves up through `Drop`.
    fn destroy(&mut self, ctx: ComponentEventContext) {}

    /// Draw editor/debug visualization for this component.
    ///
    /// This is called by editor rendering paths and should be treated as
    /// read-only visualization. It receives `&Scene`, not `&mut Scene`, by
    /// design.
    fn draw_gizmos(&self, scene: &Scene, game_object: GameObject, gizmos: &mut Gizmos) {}
}

/// Per-frame update hook, discovered via `#[reflect(ComponentUpdate)]`.
///
/// The `&self` receiver is the prototype instance from the registry, not the
/// actual component on the entity. Implementations access the real component
/// through `ctx.scene.read_component` / `ctx.scene.write_component`.
///
/// This is a separate trait, not a method on `Component`, because `update`
/// needs `&mut Scene` and would otherwise alias with `&mut self` when the
/// component lives inside the scene's ECS world.
///
/// Typical usage:
///
/// ```ignore
/// impl ComponentUpdate for PlayerController {
///     fn update(
///         &self,
///         ctx: ComponentEventContext,
///         resources: &mut ResourceMap,
///         input: &Input,
///     ) {
///         let speed = ctx
///             .scene
///             .read_component::<PlayerController, _, _>(ctx.game_object, |controller| {
///                 controller.speed
///             })
///             .unwrap_or_default();
///
///         if input.action("jump").just_pressed() {
///             // Mutate the scene or resources for this frame.
///         }
///     }
/// }
/// ```
#[reflect_trait]
pub trait ComponentUpdate: Send + Sync {
    /// Runs once per simulated frame for components whose type reflects this
    /// trait.
    ///
    /// `resources` contains global runtime state such as time and networking.
    /// `input` is the active frame input view. Update hooks are skipped when the
    /// scene manager is not simulating.
    fn update(&self, ctx: ComponentEventContext, resources: &mut ResourceMap, input: &Input);
}

/// One-time initialization hook called after a component is bound to a game object.
///
/// Discovered via `#[reflect(ComponentReset)]`. Use this to initialize related
/// scene state that depends on the owning game object existing, such as syncing
/// a rigid body from the current transform or creating helper child objects.
///
/// `reset` runs after binding, before normal per-frame updates. It may also run
/// when editor tooling rebinds or reconstructs a component, so implementations
/// should be idempotent where possible.
#[reflect_trait]
pub trait ComponentReset: Send + Sync {
    /// Initialize scene-side state for the component's owning game object.
    fn reset(&self, ctx: ComponentEventContext);
}
