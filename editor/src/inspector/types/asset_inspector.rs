use std::any::TypeId;

use engine::assets::mesh::Mesh;
use engine::core::OptionRef;
use engine::egui::Ui;
use reflect::Reflect;
use reflect::ReflectDefault;
use utils::type_ids;

use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use crate::inspector::widgets::Widgets;

#[derive(Default, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct AssetInspector;

impl TypeInspector for AssetInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        type_ids!(OptionRef<Mesh>)
    }

    fn show_inspector(&self, ui: &mut Ui, _ctx: &InspectorContext, instance: &mut dyn Reflect) {
        let instance_ptr = instance as *const _;
        if let Some(asset_ref) = instance.downcast_mut::<OptionRef<Mesh>>() {
            Widgets::asset_select(ui, instance_ptr, Some(TypeId::of::<Mesh>()), asset_ref);
        }
    }
}
