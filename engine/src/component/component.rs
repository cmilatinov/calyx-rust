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

/// Defines the lifecycle hooks for a game component.
///
/// `draw_gizmos` and `destroy` remain on this trait because they are called
/// infrequently and don't have the aliasing constraints of `update`/`reset`.
#[allow(unused)]
#[reflect_trait]
pub trait Component: TypeUuidDynamic + ComponentInstance {
    /// Called when the component is removed from a game object.
    fn destroy(&mut self, ctx: ComponentEventContext) {}
    /// Editor-only: draw debug visualization for this component.
    fn draw_gizmos(&self, scene: &Scene, game_object: GameObject, gizmos: &mut Gizmos) {}
}

/// Per-frame update hook, discovered via `#[reflect(ComponentUpdate)]`.
///
/// The `&self` receiver is the prototype instance from the registry — not the
/// actual component on the entity. Implementations access the real component
/// through `ctx.scene.read_component` / `ctx.scene.write_component`.
///
/// This is a separate trait (not on `Component`) because `update` needs
/// `&mut Scene` which would alias with `&mut self` if the component lived
/// inside the Scene's ECS World.
#[reflect_trait]
pub trait ComponentUpdate: Send + Sync {
    fn update(&self, ctx: ComponentEventContext, resources: &mut ResourceMap, input: &Input);
}

/// One-time initialization hook called when a component is bound to a game object.
/// Discovered via `#[reflect(ComponentReset)]`.
#[reflect_trait]
pub trait ComponentReset: Send + Sync {
    fn reset(&self, ctx: ComponentEventContext);
}
