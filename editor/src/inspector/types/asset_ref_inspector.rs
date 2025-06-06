use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use crate::inspector::widgets::Widgets;
use egui::Ui;
use engine::assets::animation::Animation;
use engine::assets::animation_graph::AnimationGraph;
use engine::assets::material::Material;
use engine::assets::mesh::Mesh;
use engine::assets::skybox::Skybox;
use engine::assets::texture::Texture;
use engine::assets::{AssetRef, ReflectAssetAccess};
use engine::reflect::{Reflect, ReflectDefault};
use engine::render::Shader;
use engine::type_uuids;
use engine::utils::TypeUuid;
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
            AssetRef<AnimationGraph>
        )
    }

    fn show_inspector(&self, ui: &mut Ui, ctx: &InspectorContext, instance: &mut dyn Reflect) {
        let type_registry = ctx.assets.type_registry.read();

        let Some(meta) = type_registry.trait_meta::<ReflectAssetAccess>(instance.uuid()) else {
            return;
        };
        let Some(asset_ref) = meta.get_mut(instance) else {
            return;
        };

        if Widgets::asset_select(
            ui,
            &ctx.assets.asset_registry.read(),
            ctx.field_name,
            Some(asset_ref.asset_type_uuid()),
            asset_ref.id_mut(),
        )
        .changed()
        {
            asset_ref.clear_cache();
        }
    }
}
