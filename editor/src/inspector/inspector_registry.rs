use std::ops::DerefMut;
use std::{any::TypeId, collections::HashMap};

use engine::{
    reflect::{type_registry::TypeRegistry, ReflectDefault},
    singleton, type_ids,
    utils::Init,
    uuid::Uuid,
};

use super::{
    asset_inspector::{AssetInspector, ReflectAssetInspector},
    type_inspector::{ReflectTypeInspector, TypeInspector},
};

#[derive(Default)]
pub struct InspectorRegistry {
    type_inspectors: HashMap<TypeId, Box<dyn TypeInspector>>,
    type_association: HashMap<TypeId, TypeId>,
    asset_inspectors: HashMap<Uuid, Box<dyn AssetInspector>>,
}

impl Init for InspectorRegistry {
    fn initialize(&mut self) {
        let registry = TypeRegistry::get();
        for type_id in registry.all_of(type_ids!(ReflectDefault, ReflectTypeInspector)) {
            let meta_default = registry.trait_meta::<ReflectDefault>(type_id).unwrap();
            let meta_inspector = registry
                .trait_meta::<ReflectTypeInspector>(type_id)
                .unwrap();
            let instance = meta_default.default();
            let inspector = meta_inspector.get_boxed(instance).unwrap();
            for target_type_id in inspector.target_type_ids() {
                self.type_association.insert(target_type_id, type_id);
            }
            self.type_inspectors.insert(type_id, inspector);
        }

        for type_id in registry.all_of(type_ids!(ReflectDefault, ReflectAssetInspector)) {
            let meta_default = registry.trait_meta::<ReflectDefault>(type_id).unwrap();
            let meta_inspector = registry
                .trait_meta::<ReflectAssetInspector>(type_id)
                .unwrap();
            let instance = meta_default.default();
            let inspector = meta_inspector.get_boxed(instance).unwrap();
            self.asset_inspectors
                .insert(inspector.target_type_uuid(), inspector);
        }
    }
}

singleton!(InspectorRegistry);

impl InspectorRegistry {
    pub fn type_inspector_lookup(&self, type_id: TypeId) -> Option<&dyn TypeInspector> {
        self.type_association
            .get(&type_id)
            .and_then(|id| self.type_inspectors.get(id))
            .and_then(|inspector| Some(inspector.as_ref()))
            .or_else(|| {
                self.type_inspectors
                    .get(&type_id)
                    .and_then(|inspector| Some(inspector.as_ref()))
            })
    }

    pub fn asset_inspector_lookup(&self, type_id: Uuid) -> Option<&dyn AssetInspector> {
        self.asset_inspectors
            .get(&type_id)
            .map(|inspector| inspector.as_ref())
    }
}
