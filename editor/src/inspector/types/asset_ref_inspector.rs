use engine::assets::material::Material;
use engine::assets::mesh::Mesh;
use engine::assets::texture::Texture2D;
use engine::assets::{ReflectAssetOptionRef, ReflectAssetRef};
use engine::core::{OptionRef, Ref};
use engine::egui::Ui;
use engine::render::Shader;
use engine::utils::type_uuids;
use engine::uuid::Uuid;
use reflect::Reflect;
use reflect::ReflectDefault;
use reflect::TypeUuid;

use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use crate::inspector::widgets::Widgets;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct AssetRefInspector;

impl TypeInspector for AssetRefInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        type_uuids!(
            Ref<Mesh>,
            Ref<Shader>,
            Ref<Texture2D>,
            Ref<Material>,
            OptionRef<Mesh>,
            OptionRef<Shader>,
            OptionRef<Texture2D>,
            OptionRef<Material>
        )
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
                    Some(asset_ref.asset_type_uuid()),
                    &mut asset_opt_ref,
                ) {
                    asset_ref.set(asset_opt_ref);
                }
            }
        } else if let Some(meta) = ctx.registry.trait_meta::<ReflectAssetRef>(instance.uuid()) {
            if let Some(asset_ref) = meta.get_mut(instance) {
                let mut asset = asset_ref.as_asset();
                // TODO
            }
        }
    }
}
