#![allow(missing_docs)]

use bimap::BiHashMap;
use legion::storage::ComponentTypeId;
use legion::world::{Entry, EntryMut, EntryRef, SubWorld};
use legion::World;
use std::marker::PhantomData;
use uuid::Uuid;

use calyx_engine_derive::reflect_trait;
pub use calyx_engine_derive::Component;

use crate as calyx_engine;
use crate::context::ReadOnlyRegistryContext;
use crate::input::Input;
use crate::reflect::{Reflect, TraitMeta, TraitMetaFrom};
use crate::render::Gizmos;
use crate::resource::ResourceMap;
use crate::scene::game_object_ref::RemapScope;
use crate::scene::{GameObject, Scene};
use crate::ui::{UiArena, UiNodeHandle, UiRect};
use crate::utils::{TypeUuid, TypeUuidDynamic};

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
/// #[reflect(Component, ComponentStart, ComponentUpdate, ComponentReset)]
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

    /// Splits `world` in two: the first half allows access only to this
    /// component type, the second to every other type. Both halves borrow
    /// `world` mutably, so nothing else reaches it while they live.
    fn split_world<'w>(&self, world: &'w mut World) -> (SubWorld<'w>, SubWorld<'w>);

    /// Returns an exclusive reference to this component type through an
    /// entry of a world split by [`Self::split_world`], or `None` if the
    /// entity does not have it or the entry's world cannot write it.
    fn split_instance_mut<'a>(&self, entry: EntryMut<'a>) -> Option<&'a mut dyn Component>;

    /// Remove this component type from the entity.
    fn remove_instance(&self, entry: &mut Entry);

    /// Serialize this component's current state to JSON.
    fn serialize(&self) -> Option<serde_json::Value>;

    /// Deserialize a component from JSON, returning an owned `Box<dyn Reflect>`.
    ///
    /// # Errors
    ///
    /// Returns the serde error when `value` does not match the component's
    /// serialized shape.
    fn deserialize(&self, value: &serde_json::Value)
        -> Result<Box<dyn Reflect>, serde_json::Error>;

    /// Deserializes a component like [`Self::deserialize`], with every
    /// [`GameObjectRef`](crate::scene::GameObjectRef) in it remapped through
    /// `remap`, from old object ids to new ones.
    ///
    /// The remap is entered here, in the component type's own code, because
    /// `GameObjectRef` reads it from a thread-local and a project assembly
    /// links its own copy of the engine with its own copy of that
    /// thread-local. A scope the editor entered would be invisible to a
    /// project component's deserialization, and the reverse.
    ///
    /// # Errors
    ///
    /// Returns the serde error when `value` does not match the component's
    /// serialized shape.
    fn deserialize_remapped(
        &self,
        value: &serde_json::Value,
        remap: &BiHashMap<Uuid, Uuid>,
    ) -> Result<Box<dyn Reflect>, serde_json::Error> {
        let _remap = RemapScope::enter(remap.clone());
        self.deserialize(value)
    }

    /// Convenience: deserialize JSON and assign the result to `self` in place.
    fn deserialize_in_place(&mut self, value: &serde_json::Value) -> bool {
        match self.deserialize(value) {
            Ok(value) => self.assign(value),
            Err(err) => {
                log::warn!(
                    "Failed to deserialize component {}: {err}",
                    self.type_name()
                );
                false
            }
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
/// 4. If the type reflects `ComponentStart`, `start` runs once when the scene starts.
/// 5. Each simulation frame runs `Scene::prepare`, then `Scene::update`, which
///    calls reflected `ComponentUpdate` hooks for the components the scene
///    holds enabled; see [`Component::can_disable`].
/// 6. When removed, `destroy` runs before the component leaves the object.
///
/// Physics is stepped inside scene preparation/update code before component
/// update hooks observe the frame's simulation state. Components should read
/// collision/contact results from the scene physics context during `update` and
/// write forces or transforms for the next simulation step.
///
/// `draw_gizmos` is editor/debug rendering only. It should visualize state and
/// avoid mutating gameplay data. Runtime gameplay should live in
/// `ComponentUpdate`, not in gizmo drawing.
#[allow(unused, missing_docs)]
#[reflect_trait]
pub trait Component: TypeUuidDynamic + ComponentInstance {
    /// Whether the scene can switch this component off. Defaults to `false`.
    ///
    /// A component type opts in by returning `true` once every system that
    /// consumes it skips a disabled instance, as camera selection and
    /// physics do. The scene skips the [`ComponentStart`], [`ComponentUpdate`]
    /// and [`ComponentHud`] hooks of a disabled component, so a project
    /// component whose hooks are its only consumers can opt in from its own
    /// `impl Component`. Only then does the inspector show its enabled checkbox,
    /// and only then do [`Scene::set_component_enabled`] and `calyx_ctl
    /// component disable` accept it. The method is on the prototype the
    /// component registry keeps, so callers holding only a type UUID ask
    /// [`ComponentRegistry::can_disable`](crate::class_registry::ComponentRegistry::can_disable).
    fn can_disable(&self) -> bool {
        false
    }

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

/// A typed handle to the component of type `T` on one game object, given to
/// the lifecycle hooks that also receive `&mut Scene`.
///
/// The component stays in the scene's ECS world while its hook runs, so the
/// hook reads and writes it through this handle and the scene the hook was
/// given, one access at a time. A hook therefore never holds its component
/// and the scene mutably at once, and the stored component is the only copy:
/// writes through the handle and writes through `ctx.scene`, by the hook or by
/// anything it calls, apply at once and in the order they are made, so none
/// of them can overwrite another. Once the hook removes its own component,
/// the handle reads and writes nothing; once it replaces it, the handle
/// reaches the replacement.
///
/// `ComponentRef` is `Copy`, and only names the game object. A hook can pass
/// it on, but should not keep it past the call: store a
/// [`GameObjectRef`](crate::scene::GameObjectRef) instead.
pub struct ComponentRef<T> {
    game_object: GameObject,
    _component: PhantomData<fn() -> T>,
}

impl<T> Clone for ComponentRef<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for ComponentRef<T> {}

impl<T> std::fmt::Debug for ComponentRef<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComponentRef")
            .field("game_object", &self.game_object)
            .field("component", &std::any::type_name::<T>())
            .finish()
    }
}

impl<T: Component + 'static> ComponentRef<T> {
    /// The handle to `game_object`'s component of type `T`, whether or not
    /// the object has one.
    pub fn new(game_object: GameObject) -> Self {
        Self {
            game_object,
            _component: PhantomData,
        }
    }

    /// The game object whose component this handle reaches.
    pub fn game_object(self) -> GameObject {
        self.game_object
    }

    /// Reads the component through `reader`, or returns `None` when the
    /// object no longer has it.
    pub fn read<R>(self, scene: &Scene, reader: impl FnOnce(&T) -> R) -> Option<R> {
        scene.read_component::<T, R, _>(self.game_object, reader)
    }

    /// Writes the component through `writer`, as [`Scene::write_component`]
    /// does, or returns `None` when the object no longer has it.
    pub fn write<R>(self, scene: &mut Scene, writer: impl FnOnce(&mut T) -> R) -> Option<R> {
        scene.write_component::<T, R>(self.game_object, writer)
    }

    /// Whether the object has the component.
    pub fn exists(self, scene: &Scene) -> bool {
        self.read(scene, |_| ()).is_some()
    }
}

/// One-time hook invoked when a scene begins runtime simulation.
///
/// Discovered via `#[reflect(ComponentStart)]`. The scene calls it once for
/// every game object holding the component, except one whose component the
/// scene holds disabled, before the scene's first physics preparation and
/// frame update. A component disabled when the scene starts skips the hook,
/// and does not run it later once enabled.
///
/// `this` reaches the object's own component; see [`ComponentRef`] and
/// [`ComponentUpdate`].
#[allow(missing_docs)]
pub trait ComponentStart: Component + Sized + 'static {
    fn start(this: ComponentRef<Self>, ctx: ComponentEventContext);
}

/// Per-frame update hook, discovered via `#[reflect(ComponentUpdate)]`.
///
/// The hook is an associated function, not a method: it gets no `self`.
/// `this` is a handle to the object's own component, which stays in the
/// scene's ECS world while the hook runs, because the hook also gets
/// `&mut Scene` and a `&mut self` into the world would alias it. Read and
/// write the component through `this` and `ctx.scene`; every write applies at
/// once, in the order the hook makes it (see [`ComponentRef`]).
///
/// Typical usage:
///
/// ```ignore
/// impl ComponentUpdate for PlayerController {
///     fn update(
///         this: ComponentRef<Self>,
///         ctx: ComponentEventContext,
///         resources: &mut ResourceMap,
///         input: &Input,
///     ) {
///         let Some(speed) = this.read(ctx.scene, |controller| controller.speed) else {
///             return;
///         };
///         if input.action("jump").just_pressed() {
///             this.write(ctx.scene, |controller| controller.jumps += 1);
///         }
///     }
/// }
/// ```
#[allow(missing_docs)]
pub trait ComponentUpdate: Component + Sized + 'static {
    /// Runs once per simulated frame for every game object holding the
    /// component, except one whose component the scene holds disabled.
    ///
    /// `resources` contains global runtime state such as time and networking.
    /// `input` is the active frame input view. Update hooks are skipped when the
    /// scene manager is not simulating.
    fn update(
        this: ComponentRef<Self>,
        ctx: ComponentEventContext,
        resources: &mut ResourceMap,
        input: &Input,
    );
}

/// Runtime HUD hook, discovered via `#[reflect(ComponentHud)]`.
///
/// `this` is the object's own component, borrowed from the scene's ECS world:
/// the hook only reads the scene, so the component and the scene can be
/// borrowed together. Implementations build the component's HUD into `ui`,
/// laid out for `viewport`, or return `None` to draw nothing this frame.
///
/// The hook runs once per frame after the scene update, for every game object
/// holding the component, except one whose component the scene holds
/// disabled. The player and the editor's Game panel paint the result over the
/// game view through [`SceneHud`](crate::scene::SceneHud), so the HUD reads
/// the same frame the view shows. It receives `&Scene`: gameplay belongs in
/// `ComponentUpdate`.
#[allow(missing_docs)]
pub trait ComponentHud: Component + Sized + 'static {
    fn build_hud(
        this: &Self,
        scene: &Scene,
        game_object: GameObject,
        ui: &mut UiArena,
        viewport: UiRect,
    ) -> Option<UiNodeHandle>;
}

/// One-time initialization hook called after a component is bound to a game object.
///
/// Discovered via `#[reflect(ComponentReset)]`. Use this to initialize related
/// scene state that depends on the owning game object existing, such as syncing
/// a rigid body from the current transform or creating helper child objects.
/// `this` reaches the object's own component, as in [`ComponentUpdate`].
///
/// `reset` runs after binding, before normal per-frame updates. It may also run
/// when editor tooling rebinds or reconstructs a component, so implementations
/// should be idempotent where possible.
#[allow(missing_docs)]
pub trait ComponentReset: Component + Sized + 'static {
    /// Initialize scene-side state for the component's owning game object.
    fn reset(this: ComponentRef<Self>, ctx: ComponentEventContext);
}

/// Reflected [`ComponentStart`] hook of one component type: the hook,
/// monomorphized for that type, so the scene calls it without a prototype
/// instance.
#[derive(Clone, TypeUuid)]
pub struct ReflectComponentStart {
    start: fn(ComponentEventContext),
}

impl ReflectComponentStart {
    /// Runs the hook on `ctx.game_object`'s component, when it has one.
    pub fn start(&self, ctx: ComponentEventContext) {
        (self.start)(ctx)
    }
}

impl TraitMeta for ReflectComponentStart {}

impl<T: ComponentStart> TraitMetaFrom<T> for ReflectComponentStart {
    fn trait_meta() -> Self {
        Self {
            start: |ctx| {
                let this = ComponentRef::<T>::new(ctx.game_object);
                if this.exists(ctx.scene) {
                    T::start(this, ctx);
                }
            },
        }
    }
}

/// Reflected [`ComponentUpdate`] hook of one component type; see
/// [`ReflectComponentStart`].
#[derive(Clone, TypeUuid)]
pub struct ReflectComponentUpdate {
    update: fn(ComponentEventContext, &mut ResourceMap, &Input),
}

impl ReflectComponentUpdate {
    /// Runs the hook on `ctx.game_object`'s component, when it has one.
    pub fn update(&self, ctx: ComponentEventContext, resources: &mut ResourceMap, input: &Input) {
        (self.update)(ctx, resources, input)
    }
}

impl TraitMeta for ReflectComponentUpdate {}

impl<T: ComponentUpdate> TraitMetaFrom<T> for ReflectComponentUpdate {
    fn trait_meta() -> Self {
        Self {
            update: |ctx, resources, input| {
                let this = ComponentRef::<T>::new(ctx.game_object);
                if this.exists(ctx.scene) {
                    T::update(this, ctx, resources, input);
                }
            },
        }
    }
}

/// Reflected [`ComponentHud`] hook of one component type; see
/// [`ReflectComponentStart`].
#[derive(Clone, TypeUuid)]
pub struct ReflectComponentHud {
    #[allow(clippy::type_complexity)]
    build_hud: fn(&Scene, GameObject, &mut UiArena, UiRect) -> Option<UiNodeHandle>,
}

impl ReflectComponentHud {
    /// Builds the HUD of `game_object`'s component, or returns `None` when
    /// the object has none or the hook draws nothing.
    pub fn build_hud(
        &self,
        scene: &Scene,
        game_object: GameObject,
        ui: &mut UiArena,
        viewport: UiRect,
    ) -> Option<UiNodeHandle> {
        (self.build_hud)(scene, game_object, ui, viewport)
    }
}

impl TraitMeta for ReflectComponentHud {}

impl<T: ComponentHud> TraitMetaFrom<T> for ReflectComponentHud {
    fn trait_meta() -> Self {
        Self {
            build_hud: |scene, game_object, ui, viewport| {
                scene
                    .read_component::<T, _, _>(game_object, |this| {
                        T::build_hud(this, scene, game_object, ui, viewport)
                    })
                    .flatten()
            },
        }
    }
}

/// Reflected [`ComponentReset`] hook of one component type; see
/// [`ReflectComponentStart`].
#[derive(Clone, TypeUuid)]
pub struct ReflectComponentReset {
    reset: fn(ComponentEventContext),
}

impl ReflectComponentReset {
    /// Runs the hook on `ctx.game_object`'s component, when it has one.
    pub fn reset(&self, ctx: ComponentEventContext) {
        (self.reset)(ctx)
    }
}

impl TraitMeta for ReflectComponentReset {}

impl<T: ComponentReset> TraitMetaFrom<T> for ReflectComponentReset {
    fn trait_meta() -> Self {
        Self {
            reset: |ctx| {
                let this = ComponentRef::<T>::new(ctx.game_object);
                if this.exists(ctx.scene) {
                    T::reset(this, ctx);
                }
            },
        }
    }
}
