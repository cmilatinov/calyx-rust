use engine::{
    reflect::{type_registry::TypeRegistry, ReflectDefault},
    type_uuids,
};
use std::collections::HashMap;
use uuid::Uuid;

use super::{
    asset_inspector::{AssetInspector, ReflectAssetInspector},
    type_inspector::{ReflectTypeInspector, TypeInspector},
};

#[derive(Default)]
pub struct InspectorRegistry {
    type_inspectors: HashMap<Uuid, Box<dyn TypeInspector>>,
    type_association: HashMap<Uuid, Uuid>,
    asset_inspectors: HashMap<Uuid, Box<dyn AssetInspector>>,
}

impl InspectorRegistry {
    pub fn new(registry: &TypeRegistry) -> Self {
        let mut type_inspectors = HashMap::new();
        let mut type_association = HashMap::new();
        let mut asset_inspectors = HashMap::new();
        for type_id in registry.all_of(type_uuids!(ReflectDefault, ReflectTypeInspector)) {
            let meta_default = registry.trait_meta::<ReflectDefault>(type_id).unwrap();
            let meta_inspector = registry
                .trait_meta::<ReflectTypeInspector>(type_id)
                .unwrap();
            let instance = meta_default.default();
            let inspector = meta_inspector.get_boxed(instance).unwrap();
            for target_type_id in inspector.target_type_uuids() {
                type_association.insert(target_type_id, type_id);
            }
            type_inspectors.insert(type_id, inspector);
        }

        for type_id in registry.all_of(type_uuids!(ReflectDefault, ReflectAssetInspector)) {
            let meta_default = registry.trait_meta::<ReflectDefault>(type_id).unwrap();
            let meta_inspector = registry
                .trait_meta::<ReflectAssetInspector>(type_id)
                .unwrap();
            let instance = meta_default.default();
            let inspector = meta_inspector.get_boxed(instance).unwrap();
            asset_inspectors.insert(inspector.target_type_uuid(), inspector);
        }

        Self {
            type_inspectors,
            type_association,
            asset_inspectors,
        }
    }
}

impl InspectorRegistry {
    pub fn type_inspector_lookup(&self, type_uuid: Uuid) -> Option<&dyn TypeInspector> {
        self.type_association
            .get(&type_uuid)
            .and_then(|id| self.type_inspectors.get(id))
            .and_then(|inspector| Some(inspector.as_ref()))
            .or_else(|| {
                self.type_inspectors
                    .get(&type_uuid)
                    .and_then(|inspector| Some(inspector.as_ref()))
            })
    }

    pub fn asset_inspector_lookup(&self, type_id: Uuid) -> Option<&dyn AssetInspector> {
        self.asset_inspectors
            .get(&type_id)
            .map(|inspector| inspector.as_ref())
    }
}
