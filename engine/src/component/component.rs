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
/// model, enabling serialization, reflection, and the clone-and-put-back pattern
/// used to safely call `Component::update` without aliased `&mut Scene`.
pub trait ComponentInstance: Reflect {
    /// Returns the Legion `ComponentTypeId` for the concrete component type.
    fn component_type_id(&self) -> ComponentTypeId;

    /// Returns a shared reference to this component type on the given entity,
    /// or `None` if the entity does not have it.
    fn get_instance<'a>(&self, entry: &'a EntryRef) -> Option<&'a dyn Component>;

    /// Returns an exclusive reference to this component type on the given entity,
    /// or `None` if the entity does not have it.
    fn get_instance_mut<'a>(&self, entry: &'a mut Entry) -> Option<&'a mut dyn Component>;

    /// Clone the component out of the entity as an owned trait object.
    /// The original remains in the ECS — use `put_back_instance` to overwrite
    /// it after mutation. This avoids the aliased `&mut Scene` UB that would
    /// occur if we held a `&mut Component` borrowed from inside the Scene
    /// while also passing `&mut Scene` to `Component::update`.
    fn clone_instance(&self, entry: &EntryRef) -> Option<Box<dyn Component>>;

    /// Overwrite the component on the entity with a previously cloned instance.
    /// Uses a raw pointer downcast to recover the concrete type without
    /// serialization — safe because `clone_instance` always boxes the same
    /// concrete type that this prototype was derived for.
    fn put_back_instance(&self, entry: &mut Entry, instance: Box<dyn Component>);

    /// Add a new component instance (as `Box<dyn Reflect>`) to the entity.
    /// Returns `true` if the downcast to the concrete type succeeded.
    fn bind_instance(&self, entry: &mut Entry, instance: Box<dyn Reflect>) -> bool;

    /// Remove this component type from the entity.
    fn remove_instance(&self, entry: &mut Entry);

    /// Serialize this component's current state to JSON.
    fn serialize(&self) -> Option<serde_json::Value>;

    /// Deserialize a component from JSON, returning an owned `Box<dyn Reflect>`.
    fn deserialize(&self, value: serde_json::Value) -> Option<Box<dyn Reflect>>;

    /// Convenience: deserialize JSON and assign the result to `self` in place.
    fn deserialize_in_place(&mut self, value: serde_json::Value) -> bool {
        if let Some(value) = self.deserialize(value) {
            self.assign(value)
        } else {
            false
        }
    }
}

/// Context passed to component lifecycle methods (`reset`, `update`, `destroy`).
///
/// Provides mutable access to the scene and identifies which game object the
/// component belongs to. Registry access is included for asset/type lookups.
pub struct ComponentEventContext<'a> {
    pub registries: &'a ReadOnlyRegistryContext,
    pub scene: &'a mut Scene,
    pub game_object: GameObject,
}

/// Defines the lifecycle hooks for a game component.
///
/// Lifecycle order: `reset` (once, on bind) → `update` (every frame) → `destroy` (on removal).
///
/// Components that need per-frame updates must be annotated with
/// `#[reflect_attr(update)]` so the engine includes them in the update loop.
/// `draw_gizmos` is editor-only and runs outside the normal lifecycle.
#[allow(unused)]
#[reflect_trait]
pub trait Component: TypeUuidDynamic + ComponentInstance {
    /// Called once when the component is first added to a game object.
    fn reset(&mut self, ctx: ComponentEventContext) {}
    /// Called every frame for components marked with `#[reflect_attr(update)]`.
    fn update(&mut self, ctx: ComponentEventContext, resources: &mut ResourceMap, input: &Input) {}
    /// Called when the component is removed from a game object.
    fn destroy(&mut self, ctx: ComponentEventContext) {}
    /// Editor-only: draw debug visualization for this component.
    fn draw_gizmos(&self, scene: &Scene, game_object: GameObject, gizmos: &mut Gizmos) {}
}
