use indextree::{Arena, NodeId, Node, Children};
use legion::{Entity, EntityStore, World};
use legion::world::{Entry, EntryRef};
use uuid::Uuid;

use super::error::SceneError;

use crate::assets::{AssetRegistry};
use crate::assets::mesh::Mesh;
use crate::component::{ComponentID, ComponentMesh};
use crate::component::ComponentTransform;

pub struct Scene {
    pub world: World,
    pub entity_hierarchy: Vec<NodeId>,
    entity_arena: Arena<Entity>
}

impl Default for Scene {
    fn default() -> Self {
        let mut scene = Scene {
            world: World::default(),
            entity_hierarchy: Vec::new(),
            entity_arena: Arena::new()
        };

        let mesh = AssetRegistry::get_mut().load::<Mesh>("meshes/cube").unwrap();

        let cube = scene.create_entity(None, None);
        scene.bind_component(cube, ComponentMesh { mesh: mesh.clone() }).unwrap();

        let cube2 = scene.create_entity(Some(ComponentID {
            id: Uuid::new_v4(),
            name: "Bing bong".to_string()
        }), Some(cube));
        scene.bind_component(cube2, ComponentMesh { mesh: mesh.clone() }).unwrap();

        {
            let mut e_cube = scene.entry(cube).unwrap();
            e_cube.get_component_mut::<ComponentTransform>()
                .unwrap().transform.translate(&glm::vec3(0.0, 0.0, 10.0));
        }
        {
            let mut e_cube2 = scene.entry(cube2).unwrap();
            e_cube2.get_component_mut::<ComponentTransform>()
                .unwrap().transform.translate(&glm::vec3(0.0, 5.0, 0.0));
        }

        scene
    }
}

#[allow(dead_code)]
impl Scene {
    pub fn root_entities(&self) -> &Vec<NodeId> {
        &self.entity_hierarchy
    }

    pub fn create_entity(&mut self, id: Option<ComponentID>, parent: Option<NodeId>) -> NodeId {
        let id = if let Some(id_comp) = id { id_comp }
            else { ComponentID::default() };
        let entity = self.world.push((id, ComponentTransform::default()));
        let new_node = self.entity_arena.new_node(entity);

        // push into root otherwise push under parent specified
        match parent {
            None => self.entity_hierarchy.push(new_node),
            Some(parent_id) => parent_id.append(new_node, &mut self.entity_arena)
        };

        new_node
    }

    pub fn entry(&mut self, node_id: NodeId) -> Option<Entry> {
        self.world.entry(self.get_entity(node_id)?)
    }

    pub fn entry_ref(&self, node_id: NodeId) -> Option<EntryRef> {
        self.world.entry_ref(self.get_entity(node_id)?).ok()
    }

    pub fn bind_component<T: Send + Sync + 'static>(&mut self, node_id: NodeId, component: T) -> Result<(), SceneError> {
        let mut entry = self.world.entry(self.get_entity(node_id).unwrap()).unwrap();
        entry.add_component(component);
        Ok(())
    }

    pub fn get_entity_name(&self, node_id: NodeId) -> Option<String> {
        let entry = self.entry_ref(node_id)?;
        let id_comp = entry.get_component::<ComponentID>().ok()?;
        Some(id_comp.name.clone())
    }

    pub fn get_entity_uuid(&self, node_id: NodeId) -> Option<Uuid> {
        let entry = self.entry_ref(node_id)?;
        let id_comp = entry.get_component::<ComponentID>().ok()?;
        Some(id_comp.id)
    }

    pub fn get_parent_entity(&self, node_id: NodeId) -> Option<Entity> {
        let parent_node_id = self.entity_arena.get(node_id)?.parent()?;
        Some(*self.entity_arena.get(parent_node_id)?.get())
    }

    pub fn get_parent_node(&self, node_id: NodeId) -> Option<NodeId> {
        self.entity_arena.get(node_id)?.parent()
    }

    pub fn get_node(&self, node_id: NodeId) -> Option<&Node<Entity>> {
        self.entity_arena.get(node_id)
    }

    pub fn get_entity(&self, node_id: NodeId) -> Option<Entity> {
        let node = self.entity_arena.get(node_id)?;
        Some(*node.get())
    }

    pub fn get_children(&self, node_id: NodeId) -> Children<'_, Entity> {
        node_id.children(&self.entity_arena)
    }

    pub fn get_children_count(&self, node_id: NodeId) -> usize {
        node_id.children(&self.entity_arena).into_iter().count()
    }
}
