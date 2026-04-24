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
/// model, enabling serialization and reflection.
pub trait ComponentInstance: Reflect {
    /// Returns the Legion `ComponentTypeId` for the concrete component type.
    fn component_type_id(&self) -> ComponentTypeId;

    /// Returns a shared reference to this component type on the given entity,
    /// or `None` if the entity does not have it.
    fn get_instance<'a>(&self, entry: &'a EntryRef) -> Option<&'a dyn Component>;

    /// Returns an exclusive reference to this component type on the given entity,
    /// or `None` if the entity does not have it.
    fn get_instance_mut<'a>(&self, entry: &'a mut Entry) -> Option<&'a mut dyn Component>;

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

/// Context passed to component lifecycle functions.
///
/// Provides mutable access to the scene and identifies which game object the
/// component belongs to. Registry access is included for asset/type lookups.
pub struct ComponentEventContext<'a> {
    pub registries: &'a ReadOnlyRegistryContext,
    pub scene: &'a mut Scene,
    pub game_object: GameObject,
}

/// Type-erased function pointer for per-frame component updates.
///
/// Registered in `ComponentRegistry` by `#[derive(Component)]` for types
/// annotated with `#[reflect_attr(update)]`. The function accesses its own
/// component state through `scene.write_component::<T>(game_object, ...)`.
pub type ComponentUpdateFn = fn(ComponentEventContext, &mut ResourceMap, &Input);

/// Type-erased function pointer for component initialization on bind.
///
/// Registered in `ComponentRegistry` by `#[derive(Component)]` for types
/// that implement a `reset` function.
pub type ComponentResetFn = fn(ComponentEventContext);

/// Defines the lifecycle hooks for a game component.
///
/// `update` and `reset` are dispatched via registered function pointers
/// (see `ComponentUpdateFn` / `ComponentResetFn`) rather than trait methods,
/// because they need `&mut Scene` which would alias with `&mut self` if the
/// component lives inside the Scene's ECS World.
///
/// `draw_gizmos` and `destroy` remain on the trait because they are called
/// infrequently and don't have the same aliasing constraints.
#[allow(unused)]
#[reflect_trait]
pub trait Component: TypeUuidDynamic + ComponentInstance {
    /// Called when the component is removed from a game object.
    fn destroy(&mut self, ctx: ComponentEventContext) {}
    /// Editor-only: draw debug visualization for this component.
    fn draw_gizmos(&self, scene: &Scene, game_object: GameObject, gizmos: &mut Gizmos) {}
}
