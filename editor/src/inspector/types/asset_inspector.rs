use std::any::TypeId;
use engine::assets::mesh::Mesh;
use engine::assets::ReflectAssetRef;
use engine::core::Ref;
use engine::egui::Ui;
use reflect::Reflect;
use reflect::ReflectDefault;
use reflect::registry::TypeRegistry;
use utils::type_ids;
use crate::inspector::type_inspector::{TypeInspector, ReflectTypeInspector};

#[derive(Default, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct AssetInspector;

impl TypeInspector for AssetInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        type_ids!(Ref<Mesh>)
    }

    fn show_inspector(&self, _ui: &mut Ui, registry: &TypeRegistry, instance: &mut dyn Reflect) {
        let id = instance.as_any().type_id();
        let meta = registry.trait_meta::<ReflectAssetRef>(id).unwrap();
        let _asset = meta.get(instance).unwrap().as_asset();
    }
}