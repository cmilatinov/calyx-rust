use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::scene::Scene;
use calyx_engine::utils::TypeUuid;
use uuid::Uuid;

use crate::asset_open::{
    AssetOpenContext, AssetOpenOutcome, AssetOpener, ReflectAssetOpener, SceneOpenTarget,
};
use crate::inspector::asset_inspector::{AssetInspector, ReflectAssetInspector};

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, AssetInspector, AssetOpener)]
pub struct SceneInspector;

impl AssetInspector for SceneInspector {
    fn target_type_uuid(&self) -> Uuid {
        Scene::type_uuid()
    }
}

/// A scene opens as the authoring scene, as File > Open does.
impl AssetOpener for SceneInspector {
    fn target_type_uuid(&self) -> Uuid {
        Scene::type_uuid()
    }

    fn open_unavailable_reason(
        &self,
        editor: &AssetOpenContext<'_>,
        _asset_id: Uuid,
    ) -> Option<&'static str> {
        if !editor.assemblies_loaded() {
            return Some("Build project assemblies before opening scenes");
        }
        editor.scene_switch_refusal()
    }

    fn open(&self, editor: &mut AssetOpenContext<'_>, asset_id: Uuid) -> AssetOpenOutcome {
        editor.request_scene_open(SceneOpenTarget::Asset(asset_id))
    }
}
