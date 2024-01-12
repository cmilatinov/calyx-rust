use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::ops::Deref;
use std::path::PathBuf;
use indextree::NodeId;
use legion::{Entity, EntityStore, IntoQuery};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::class_registry::ClassRegistry;
use crate::component::ComponentID;
use crate::scene::{Scene, SceneError};

#[derive(Serialize)]
pub struct Prefab {
    components: HashMap<Uuid, serde_json::Value>,
    id: Uuid,

    #[serde(skip_serializing)]
    node_id: NodeId,

    #[serde(skip_serializing)]
    path: PathBuf
}

impl Prefab {
    pub fn new(scene: &Scene, node_id: NodeId) -> Result<Prefab, SceneError> {
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name("prefab.json")
            .add_filter("JSON", &["json"])
            .save_file() {
            let world = scene.world();
            let mut query = <(Entity, &ComponentID)>::query();
            let mut components: HashMap<Uuid, serde_json::Value> = HashMap::new();

            for (entity, id) in query.iter(world.deref()) {
                if scene.get_node(*entity) != node_id {
                    continue;
                }

                for (component_id, component) in ClassRegistry::get().components_uuid() {
                    let entry = world.entry_ref(*entity).unwrap();
                    if let Some(instance) = component.get_instance(&entry) {
                        if let Some(value) = instance.serialize() {
                            components
                                .insert(*component_id, value);
                        }
                    }
                }

                let prefab = Prefab {
                    components,
                    id: id.id,
                    node_id,
                    path: path.clone(),
                };

                let json_data = serde_json::to_string_pretty(&prefab).unwrap();
                let mut file = File::create(path.clone()).unwrap();
                file.write_all(json_data.as_bytes()).unwrap();

                return Ok(prefab)
            }
        }

        Err(SceneError::UnableToCreatePrefab)
    }
    pub fn save(&mut self, scene: &Scene) -> Result<(), SceneError> {
        if !self.path.exists() {
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name("prefab.json")
                .add_filter("JSON", &["json"])
                .save_file() {
                self.path = path;
            }
        }

        let world = scene.world();
        let mut query = <(Entity, &ComponentID)>::query();
        let mut components: HashMap<Uuid, serde_json::Value> = HashMap::new();

        for (entity, id) in query.iter(world.deref()) {
            if scene.get_node(*entity) != self.node_id {
                continue;
            }

            for (component_id, component) in ClassRegistry::get().components_uuid() {
                let entry = world.entry_ref(*entity).unwrap();
                if let Some(instance) = component.get_instance(&entry) {
                    if let Some(value) = instance.serialize() {
                        components
                            .insert(*component_id, value);
                    }
                }
            }

            self.components = components;

            let json_data = serde_json::to_string_pretty(&self).unwrap();
            let mut file = File::create(self.path.clone()).unwrap();
            file.write_all(json_data.as_bytes()).unwrap();

            return Ok(())
        }

        Err(SceneError::UnableToCreatePrefab)
    }
    pub fn open(path: PathBuf, scene: &mut Scene) -> Result<Prefab, SceneError> {
        if path.exists() {
            return Err(SceneError::UnableToCreatePrefab);
        }

        #[derive(Deserialize)]
        struct TempPrefab {
            components: HashMap<Uuid, serde_json::Value>,
            id: Uuid,
        }

        let mut file = File::open(path.clone()).unwrap();
        let mut contents = Default::default();
        file.read_to_string(&mut contents).unwrap();
        let node = scene.new_entity(None);
        let tmp: TempPrefab = serde_json::from_str(&contents).unwrap();
        let prefab = Prefab {
            components: tmp.components,
            id: tmp.id,
            node_id: node,
            path
        };

        for (component_id, data) in &prefab.components {
            if let Some(component) = ClassRegistry::get().component_by_uuid(component_id.clone()) {
                if let Some(instance) = component.deserialize(data.clone()) {
                    if let Some(mut entry) = scene.world_mut().entry(scene.get_entity(node)) {
                        let _ = component.bind_instance(&mut entry, instance);
                    }
                }
            }
        }

        Ok(prefab)
    }
}
