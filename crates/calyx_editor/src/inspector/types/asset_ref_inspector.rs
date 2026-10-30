use crate::inspector::type_inspector::{
    InspectorContext, InspectorResponse, ReflectTypeInspector, TypeInspector,
};
use crate::inspector::widgets::Widgets;
use calyx_engine::assets::animation::Animation;
use calyx_engine::assets::animation_graph::AnimationGraph;
use calyx_engine::assets::material::Material;
use calyx_engine::assets::mesh::Mesh;
use calyx_engine::assets::skybox::Skybox;
use calyx_engine::assets::texture::Texture;
use calyx_engine::assets::{AssetRef, ReflectAssetAccess};
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::render::Shader;
use calyx_engine::scene::Prefab;
use calyx_engine::type_uuids;
use calyx_engine::utils::TypeUuid;
use egui::Ui;
use uuid::Uuid;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct AssetRefInspector;

impl TypeInspector for AssetRefInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        type_uuids!(
            AssetRef<Mesh>,
            AssetRef<Shader>,
            AssetRef<Texture>,
            AssetRef<Material>,
            AssetRef<Skybox>,
            AssetRef<Animation>,
            AssetRef<AnimationGraph>,
            AssetRef<Prefab>
        )
    }

    fn show_inspector(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
    ) -> InspectorResponse {
        let type_registry = ctx.assets.types.read();

        let Some(meta) = type_registry.trait_meta::<ReflectAssetAccess>(instance.uuid()) else {
            return InspectorResponse::NONE;
        };
        let Some(asset_ref) = meta.get_mut(instance) else {
            return InspectorResponse::NONE;
        };

        let response = Widgets::asset_select(
            ui,
            &ctx.assets.assets.read(),
            ctx.field_name,
            Some(asset_ref.asset_type_uuid()),
            asset_ref.id_mut(),
        );
        if response.changed() {
            asset_ref.clear_cache();
        }
        InspectorResponse::from_response(&response)
    }
}
