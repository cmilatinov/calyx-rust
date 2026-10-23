use legion::world::{EntryRef, SubWorld};
use legion::{Entity, EntityStore, World};
use uuid::Uuid;

use crate::component::ComponentID;
use crate::scene::game_object_store::GameObjectStore;
use crate::scene::scene_graph::SceneGraph;
use crate::scene::GameObject;

/// A read-only view of a scene's objects: their names, ids and hierarchy.
///
/// [`Scene::view`](crate::scene::Scene::view) views the whole scene.
/// [`Scene::edit_component_dyn`](crate::scene::Scene::edit_component_dyn)
/// hands its editor a view of every component but the edited one's type, so
/// the view cannot read that type on any object. When the edited type is
/// [`ComponentID`], [`SceneView::name`] knows no names.
pub struct SceneView<'a> {
    world: ViewWorld<'a>,
    graph: &'a SceneGraph,
    store: &'a GameObjectStore,
    root: GameObject,
}

/// The components a [`SceneView`] reads: the whole world, or the half of a
/// split world that excludes an edited component type.
enum ViewWorld<'a> {
    Whole(&'a World),
    Split(SubWorld<'a>),
}

impl<'a> SceneView<'a> {
    pub(crate) fn whole(
        world: &'a World,
        graph: &'a SceneGraph,
        store: &'a GameObjectStore,
        root: GameObject,
    ) -> Self {
        Self {
            world: ViewWorld::Whole(world),
            graph,
            store,
            root,
        }
    }

    pub(crate) fn split(
        world: SubWorld<'a>,
        graph: &'a SceneGraph,
        store: &'a GameObjectStore,
        root: GameObject,
    ) -> Self {
        Self {
            world: ViewWorld::Split(world),
            graph,
            store,
            root,
        }
    }

    /// Iterates every non-root object in the scene.
    pub fn objects(&self) -> impl Iterator<Item = GameObject> + 'a {
        self.graph.objects(self.root)
    }

    /// Returns the display name stored in [`ComponentID`] for `game_object`,
    /// or `None` when the object is unknown or the view cannot read its
    /// [`ComponentID`].
    pub fn name(&self, game_object: GameObject) -> Option<String> {
        self.world
            .entry_ref(game_object.entity)?
            .get_component::<ComponentID>()
            .ok()
            .map(|id| id.name.clone())
    }

    /// Returns the persistent UUID `game_object` was spawned with, or the nil
    /// UUID for an unknown object. It is known even when the view cannot read
    /// [`ComponentID`].
    pub fn uuid(&self, game_object: GameObject) -> Uuid {
        self.store.uuid(game_object.entity).unwrap_or_default()
    }

    /// Resolves a game object by persistent UUID.
    pub fn find(&self, id: Uuid) -> Option<GameObject> {
        self.store.find(id)
    }

    /// Returns the direct parent of `game_object`, if any.
    pub fn parent(&self, game_object: GameObject) -> Option<GameObject> {
        self.graph.parent(game_object)
    }
}

impl ViewWorld<'_> {
    fn entry_ref(&self, entity: Entity) -> Option<EntryRef<'_>> {
        match self {
            Self::Whole(world) => world.entry_ref(entity).ok(),
            Self::Split(world) => world.entry_ref(entity).ok(),
        }
    }
}
