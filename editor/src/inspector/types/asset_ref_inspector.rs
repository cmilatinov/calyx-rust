use engine::assets::animation::Animation;
use engine::assets::material::Material;
use engine::assets::mesh::Mesh;
use engine::assets::skybox::Skybox;
use engine::assets::texture::Texture;
use engine::assets::{ReflectAssetOptionRef, ReflectAssetRef};
use engine::core::Ref;
use engine::egui::Ui;
use engine::reflect::{Reflect, ReflectDefault};
use engine::render::Shader;
use engine::type_ids;
use engine::utils::TypeUuid;
use std::any::TypeId;

use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use crate::inspector::widgets::Widgets;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct AssetRefInspector;

impl TypeInspector for AssetRefInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        type_ids!(
            Ref<Mesh>,
            Ref<Shader>,
            Ref<Texture>,
            Ref<Material>,
            Ref<Animation>,
            Ref<Skybox>,
            Option<Ref<Mesh>>,
            Option<Ref<Shader>>,
            Option<Ref<Texture>>,
            Option<Ref<Material>>,
            Option<Ref<Animation>>,
            Option<Ref<Skybox>>
        )
    }

    fn show_inspector(&self, ui: &mut Ui, ctx: &InspectorContext, instance: &mut dyn Reflect) {
        if let Some(meta) = ctx
            .registry
            .trait_meta::<ReflectAssetOptionRef>(instance.as_any().type_id())
        {
            if let Some(asset_ref) = meta.get_mut(instance) {
                let mut asset_opt_ref = asset_ref.as_asset_option();
                if Widgets::asset_select(
                    ui,
                    ctx.field_name,
                    Some(asset_ref.asset_type_uuid()),
                    &mut asset_opt_ref,
                ) {
                    asset_ref.set(asset_opt_ref);
                }
            }
        } else if let Some(meta) = ctx
            .registry
            .trait_meta::<ReflectAssetRef>(instance.as_any().type_id())
        {
            if let Some(asset_ref) = meta.get_mut(instance) {
                let mut _asset = asset_ref.as_asset();
                // TODO
            }
        }
    }
}
