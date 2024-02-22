use std::collections::{HashMap, HashSet};
use std::io::BufReader;
use std::path::Path;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use egui::Ui;
use generational_indextree::{Arena, NodeId};
use glm::Mat4;
use legion::world::{Entry, EntryRef};
use legion::{Entity, EntityStore, IntoQuery, World};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate as engine;
use crate::assets::error::AssetError;
use crate::assets::{Asset, LoadedAsset};
use crate::class_registry::ClassRegistry;
use crate::component::{Component, ComponentTransform};
use crate::component::{ComponentCamera, ComponentID};
use crate::math::Transform;
use crate::scene::Prefab;
use crate::utils::TypeUuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GameObject {
    pub node: NodeId,
    pub entity: Entity,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct SceneData {
    pub components: HashMap<Uuid, HashMap<Uuid, serde_json::Value>>,
    pub hierarchy: HashMap<Uuid, Uuid>,
}

#[derive(Default, Debug, TypeUuid)]
#[uuid = "9946a2e7-e022-447e-8e60-528da548087f"]
pub struct Scene {
    pub world: World,
    root_objects: HashSet<GameObject>,
    uuid_map: HashMap<Uuid, GameObject>,
    entity_map: HashMap<Entity, NodeId>,
    entity_arena: Arena<Entity>,
    transform_cache: RwLock<HashMap<NodeId, Transform>>,
    camera: Option<GameObject>,
    objects_to_delete: HashSet<GameObject>,
}

impl Asset for Scene {
    fn get_file_extensions() -> &'static [&'static str]
    where
        Self: Sized,
    {
        &["cxscene"]
    }

    fn from_file(path: &Path) -> Result<LoadedAsset<Self>, AssetError>
    where
        Self: Sized,
    {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .open(path)
            .map_err(|_| AssetError::LoadError)?;

        let reader = BufReader::new(file);
        Ok(LoadedAsset::new(
            serde_json::from_reader(reader).map_err(|_| AssetError::LoadError)?,
        ))
    }
}

impl Serialize for Scene {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let data: SceneData = self.into();
        data.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Scene {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Scene::from(SceneData::deserialize(deserializer)?))
    }
}

impl Clone for Scene {
    fn clone(&self) -> Self {
        let data: SceneData = self.into();
        data.into()
    }
}

impl From<SceneData> for Scene {
    fn from(value: SceneData) -> Self {
        let mut scene = Self::default();
        for (_, components) in value.components {
            let game_object = scene.new_game_object(None);
            for (component_id, data) in components {
                if let Some(component) = ClassRegistry::get().component_by_uuid(component_id) {
                    if let Some(instance) = component.deserialize(data) {
                        if let Some(mut entry) = scene.entry_mut(game_object) {
                            let _ = component.bind_instance(&mut entry, instance);
                        }
                    }
                }
            }
            let mut id = None;
            if let Some(entry) = scene.entry(game_object) {
                if let Ok(c_id) = entry.get_component::<ComponentID>() {
                    id = Some(c_id.id);
                }
            }
            if let Some(id) = id {
                scene.uuid_map.insert(id, game_object);
            }
        }
        for (id, parent) in value.hierarchy {
            if let Some(game_object) = scene.get_game_object_by_uuid(id) {
                if let Some(parent) = scene.get_game_object_by_uuid(parent) {
                    scene.set_parent(game_object, Some(parent));
                }
            }
        }
        scene
    }
}

impl From<&Scene> for SceneData {
    fn from(scene: &Scene) -> Self {
        let world = &scene.world;
        let mut query = <(Entity, &ComponentID)>::query();
        let mut hierarchy = HashMap::new();
        let mut components: HashMap<Uuid, HashMap<Uuid, serde_json::Value>> = HashMap::new();
        for (entity, id) in query.iter(world) {
            if let Some(game_object) = scene.get_game_object_from_entity(*entity) {
                if let Some(parent) = scene.get_parent_game_object(game_object) {
                    if let Some(entry) = scene.entry(parent) {
                        if let Ok(parent_id) = entry.get_component::<ComponentID>() {
                            hierarchy.insert(id.id, parent_id.id);
                        }
                    }
                }
                for (component_id, component) in ClassRegistry::get().components_uuid() {
                    if let Some(entry) = scene.entry(game_object) {
                        if let Some(instance) = component.get_instance(&entry) {
                            if let Some(value) = instance.serialize() {
                                components
                                    .entry(id.id)
                                    .or_default()
                                    .insert(*component_id, value);
                            }
                        }
                    }
                }
            }
        }
        SceneData {
            hierarchy,
            components,
        }
    }
}

impl From<(&Scene, GameObject)> for SceneData {
    fn from((scene, game_object): (&Scene, GameObject)) -> Self {
        let mut hierarchy = HashMap::new();
        let mut components: HashMap<Uuid, HashMap<Uuid, serde_json::Value>> = HashMap::new();

        game_object
            .node
            .descendants(&scene.entity_arena)
            .filter_map(|c| scene.get_game_object_from_node(c))
            .for_each(|game_object| {
                if let Some(entry) = scene.entry(game_object) {
                    if let Ok(id) = entry.get_component::<ComponentID>() {
                        if let Some(parent) = scene.get_parent_game_object(game_object) {
                            if let Some(entry) = scene.entry(parent) {
                                if let Ok(parent_id) = entry.get_component::<ComponentID>() {
                                    hierarchy.insert(id.id, parent_id.id);
                                }
                            }
                        }
                        for (component_id, component) in ClassRegistry::get().components_uuid() {
                            if let Some(entry) = scene.entry(game_object) {
                                if let Some(instance) = component.get_instance(&entry) {
                                    if let Some(value) = instance.serialize() {
                                        components
                                            .entry(id.id)
                                            .or_default()
                                            .insert(*component_id, value);
                                    }
                                }
                            }
                        }
                    }
                }
            });

        SceneData {
            hierarchy,
            components,
        }
    }
}

impl Scene {
    pub(crate) fn get_game_object_from_node(&self, node: NodeId) -> Option<GameObject> {
        self.entity_arena.get(node).map(|n| GameObject {
            node,
            entity: *n.get(),
        })
    }

    pub(crate) fn get_game_object_from_entity(&self, entity: Entity) -> Option<GameObject> {
        self.entity_map.get(&entity).map(|node| GameObject {
            node: *node,
            entity,
        })
    }
}

#[allow(dead_code)]
impl Scene {
    pub fn root_objects(&self) -> &HashSet<GameObject> {
        &self.root_objects
    }

    pub fn create_game_object(
        &mut self,
        id: Option<ComponentID>,
        parent: Option<GameObject>,
    ) -> GameObject {
        let c_id = if let Some(id_comp) = id {
            id_comp
        } else {
            ComponentID::default()
        };
        let id = c_id.id;
        let entity = self.world.push((c_id, ComponentTransform::default()));
        let node = self.entity_arena.new_node(entity);
        let game_object = GameObject { node, entity };
        self.entity_map.insert(entity, node);
        self.uuid_map.insert(id, game_object);

        // push into root otherwise push under parent specified
        match parent {
            None => {
                self.root_objects.insert(game_object);
            }
            Some(parent) => {
                parent.node.append(game_object.node, &mut self.entity_arena);
            }
        };

        game_object
    }

    pub fn delete_game_object(&mut self, game_object: GameObject) {
        self.objects_to_delete.insert(game_object);
    }

    pub fn create_prefab(&self, game_object: GameObject) -> Prefab {
        let data: SceneData = (self, game_object).into();

        Prefab {
            data: data.clone(),
            scene: data.into(),
        }
    }

    pub fn instantiate_prefab(&mut self, prefab: &Prefab, parent: Option<GameObject>) {
        let root_node = prefab.scene.root_objects().iter().next().unwrap();

        for (_, components) in prefab.data.components.iter() {
            let game_object = self.new_game_object(None);
            for (component_id, data) in components {
                if let Some(component) = ClassRegistry::get().component_by_uuid(*component_id) {
                    if let Some(instance) = component.deserialize(data.clone()) {
                        if let Some(mut entry) = self.entry_mut(game_object) {
                            let _ = component.bind_instance(&mut entry, instance);
                        }
                    }
                }
            }
            let mut id = None;
            if let Some(entry) = self.entry(game_object) {
                if let Ok(c_id) = entry.get_component::<ComponentID>() {
                    id = Some(c_id.id);
                }
            }
            if let Some(id) = id {
                self.uuid_map.insert(id, game_object);
            }
        }

        for (id, parent) in prefab.data.hierarchy.iter() {
            if let Some(game_object) = self.get_game_object_by_uuid(*id) {
                if let Some(parent) = self.get_game_object_by_uuid(*parent) {
                    self.set_parent(game_object, Some(parent));
                }
            }
        }

        // for (id, _) in prefab.data.components.iter() {
        //     if let Some(node) = self.get_node_by_uuid(*id) {
        //         self.world
        //             .entry(self.get_entity(node))
        //             .unwrap()
        //             .get_component_mut::<ComponentID>()
        //             .unwrap()
        //             .id = Uuid::new_v4();
        //     }
        // }

        if let Some(parent) = parent {
            if let Some(game_object) =
                self.get_game_object_by_uuid(prefab.scene.get_game_object_uuid(*root_node))
            {
                self.set_parent(game_object, Some(parent));
            }
        }
    }

    pub fn set_parent(&mut self, game_object: GameObject, parent: Option<GameObject>) {
        if let Some(_) = self.get_parent_game_object(game_object) {
            game_object.node.detach(&mut self.entity_arena);
        }
        if let Some(parent) = parent {
            parent.node.append(game_object.node, &mut self.entity_arena);
            self.root_objects.remove(&game_object);
        } else {
            self.root_objects.insert(game_object);
        }
    }

    pub fn get_main_camera<'a>(
        &'a self,
        world: &'a World,
    ) -> Option<(GameObject, &'a ComponentCamera)> {
        let mut query = <(Entity, &ComponentTransform, &ComponentCamera)>::query();
        query
            .iter(world)
            .filter_map(|(e, t, c)| self.get_game_object_from_entity(*e).map(|go| (go, t, c)))
            .find(|(go, _, c)| {
                if let Some(camera) = &self.camera {
                    go == camera
                } else {
                    c.enabled
                }
            })
            .map(|(go, _, c)| (go, c))
    }

    pub(crate) fn new_game_object(&mut self, parent: Option<GameObject>) -> GameObject {
        let entity = self.world.push(());
        let node = self.entity_arena.new_node(entity);
        let game_object = GameObject { node, entity };
        self.entity_map.insert(entity, node);
        match parent {
            None => {
                self.root_objects.insert(game_object);
            }
            Some(parent) => {
                parent.node.append(node, &mut self.entity_arena);
            }
        };
        game_object
    }

    fn transform_cache(&self) -> RwLockReadGuard<HashMap<NodeId, Transform>> {
        self.transform_cache.read().unwrap()
    }

    fn transform_cache_mut(&self) -> RwLockWriteGuard<HashMap<NodeId, Transform>> {
        self.transform_cache.write().unwrap()
    }

    pub fn bind_component<T: Send + Sync + 'static>(
        &mut self,
        game_object: GameObject,
        component: T,
    ) -> Option<()> {
        self.entry_mut(game_object)
            .map(|mut e| e.add_component(component))
    }

    pub fn get_component_ptr(
        &mut self,
        game_object: GameObject,
        component: &Box<dyn Component>,
    ) -> Option<*mut dyn Component> {
        self.entry_mut(game_object).and_then(|mut entry| {
            if let Some(instance) = component.get_instance_mut(&mut entry) {
                Some(instance as *mut dyn Component)
            } else {
                None
            }
        })
    }

    pub fn entry(&self, game_object: GameObject) -> Option<EntryRef> {
        self.world.entry_ref(game_object.entity).ok()
    }

    pub fn entry_mut(&mut self, game_object: GameObject) -> Option<Entry> {
        self.world.entry(game_object.entity)
    }

    pub fn update(&mut self, ui: &Ui) {
        for (_, component) in ClassRegistry::get().components_update() {
            // No way around this, we want component's update method to take &mut self
            // but there's no way to do that and provide an &mut Scene
            // At worst, this is a race condition because we can guarantee that
            // this reference only lives until the end of this function
            let scene = unsafe { &mut *(self as *mut Self) };
            let game_objects = <Entity>::query()
                .iter(&self.world)
                .filter_map(|e| self.get_game_object_from_entity(*e))
                .collect::<Vec<_>>();
            for game_object in game_objects {
                if let Some(mut entry) = self.entry_mut(game_object) {
                    if let Some(instance) = component.get_instance_mut(&mut entry) {
                        instance.update(scene, game_object, ui);
                    }
                }
            }
        }
        self.delete_game_objects();
    }

    pub fn delete_game_objects(&mut self) {
        let mut objects_to_delete = HashSet::new();
        std::mem::swap(&mut objects_to_delete, &mut self.objects_to_delete);
        for game_object in objects_to_delete {
            if self.get_parent_game_object(game_object).is_none() {
                self.root_objects.remove(&game_object);
            }
            for go in self.get_children(game_object).collect::<Vec<_>>() {
                self.world.remove(go.entity);
                self.entity_map.remove(&go.entity);
                self.uuid_map.remove(&self.get_game_object_uuid(go));
            }
            game_object.node.remove_subtree(&mut self.entity_arena);
        }
    }

    pub fn get_game_object_name(&self, game_object: GameObject) -> String {
        self.entry(game_object)
            .and_then(|e| {
                e.get_component::<ComponentID>()
                    .ok()
                    .map(|c| c.name.clone())
            })
            .unwrap_or_default()
    }

    pub fn get_game_object_uuid(&self, game_object: GameObject) -> Uuid {
        self.entry(game_object)
            .and_then(|e| e.get_component::<ComponentID>().ok().map(|id| id.id))
            .unwrap_or_default()
    }

    pub fn get_game_object_by_uuid(&self, id: Uuid) -> Option<GameObject> {
        self.uuid_map.get(&id).copied()
    }

    pub fn get_parent_game_object(&self, game_object: GameObject) -> Option<GameObject> {
        self.entity_arena
            .get(game_object.node)?
            .parent()
            .and_then(|node| self.get_game_object_from_node(node))
    }

    pub fn get_children<'a>(
        &'a self,
        game_object: GameObject,
    ) -> impl Iterator<Item = GameObject> + 'a {
        game_object
            .node
            .children(&self.entity_arena)
            .filter_map(|node| self.get_game_object_from_node(node))
    }

    pub fn get_children_count(&self, game_object: GameObject) -> usize {
        self.get_children(game_object).count()
    }

    pub fn get_transform(&self, game_object: GameObject) -> Transform {
        if let Some(entry) = self.entry(game_object) {
            if let Ok(c_transform) = entry.get_component::<ComponentTransform>() {
                return c_transform.transform;
            }
        }
        Transform::default()
    }

    pub fn set_transform(&mut self, game_object: GameObject, matrix: Mat4) {
        if let Some(mut entry) = self.entry_mut(game_object) {
            if let Ok(tc) = entry.get_component_mut::<ComponentTransform>() {
                tc.transform.set_local_matrix(&matrix);
            }
        }
    }

    pub fn set_world_transform(&mut self, game_object: GameObject, matrix: Mat4) {
        let parent_transform = self
            .get_parent_game_object(game_object)
            .map_or(Mat4::identity(), |go| {
                self.get_world_transform(go).inverse_matrix
            });
        if let Some(mut entry) = self.entry_mut(game_object) {
            if let Ok(tc) = entry.get_component_mut::<ComponentTransform>() {
                tc.transform.set_local_matrix(&(parent_transform * matrix));
            }
        }
    }

    pub fn get_world_transform(&self, game_object: GameObject) -> Transform {
        if let Some(transform) = self.transform_cache().get(&game_object.node) {
            return *transform;
        }
        let entry = self.entry(game_object);
        let mut matrix = entry
            .as_ref()
            .map(|e| e.get_component::<ComponentTransform>().ok())
            .map_or(Mat4::identity(), |co| {
                co.map_or(Mat4::identity(), |c| c.transform.matrix)
            });
        if let Some(parent_node) = self.get_parent_game_object(game_object) {
            matrix = self.get_world_transform(parent_node).matrix * matrix;
        }
        let transform = matrix.into();
        self.transform_cache_mut()
            .insert(game_object.node, transform);
        transform
    }

    pub fn clear_transform_cache(&self) {
        self.transform_cache_mut().clear();
    }

    pub fn get_children_with_component<'a, T: Component>(
        &'a self,
        game_object: GameObject,
    ) -> impl Iterator<Item = GameObject> + 'a {
        game_object
            .node
            .descendants(&self.entity_arena)
            .filter_map(|c| match self.get_game_object_from_node(c) {
                Some(go) => self.entry(go).and_then(|e| {
                    if e.get_component::<T>().is_ok() {
                        Some(go)
                    } else {
                        None
                    }
                }),
                None => None,
            })
    }
}
