use std::any::TypeId;

use engine::assets::mesh::Mesh;
use engine::assets::ReflectAssetOptionRef;
use engine::core::OptionRef;
use engine::egui::Ui;
use engine::utils::type_uuids;
use engine::uuid::Uuid;
use reflect::Reflect;
use reflect::ReflectDefault;
use reflect::TypeUuid;

use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use crate::inspector::widgets::Widgets;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct AssetInspector;

impl TypeInspector for AssetInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        type_uuids!(OptionRef<Mesh>)
    }

    fn show_inspector(&self, ui: &mut Ui, ctx: &InspectorContext, instance: &mut dyn Reflect) {
        if let Some(meta) = ctx
            .registry
            .trait_meta::<ReflectAssetOptionRef>(instance.uuid())
        {
            if let Some(asset_ref) = meta.get_mut(instance) {
                let mut asset_opt_ref = asset_ref.as_asset_option();
                if Widgets::asset_select(
                    ui,
                    ctx.node,
                    Some(TypeId::of::<Mesh>()),
                    &mut asset_opt_ref,
                ) {
                    asset_ref.set(asset_opt_ref);
                }
            }
        }
    }
}
