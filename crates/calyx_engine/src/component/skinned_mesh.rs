use super::{Component, ComponentBone, ReflectComponent};
use crate as calyx_engine;
use crate::assets::material::Material;
use crate::assets::mesh::{BoneTransform, Mesh};
use crate::assets::AssetRef;
use crate::reflect::{Reflect, ReflectDefault};
use crate::scene::{GameObject, GameObjectRef, Scene};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra_glm::Mat4;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Mesh renderer component driven by a bone hierarchy.
#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "bb784426-a5ec-4995-a86a-c40e7e2cb3ab"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Skinned Mesh Renderer")]
#[serde(default)]
pub struct ComponentSkinnedMesh {
    /// Material asset applied to the mesh.
    pub material: AssetRef<Material>,
    /// Skinned mesh asset.
    pub mesh: AssetRef<Mesh>,
    /// Root bone object for the skeleton.
    pub root_bone: GameObjectRef,
    /// Runtime bone matrices uploaded to the renderer.
    #[reflect_skip]
    #[serde(skip)]
    pub bone_transforms: Vec<BoneTransform>,
}

impl Component for ComponentSkinnedMesh {}

impl ComponentSkinnedMesh {
    /// Root-space transform of each bone object under `root`, by bone name, as the scene holds
    /// them. Reads the scene only, so it poses a skinned mesh without running its animator.
    pub fn bone_poses(scene: &Scene, root: GameObject) -> HashMap<String, Mat4> {
        let root_inverse = scene.world_transform(root).inverse_matrix();
        scene
            .descendants_with::<ComponentBone>(root)
            .filter_map(|bone| {
                let name = scene.read_component::<ComponentBone, _, _>(bone, |c| c.name.clone())?;
                Some((name, root_inverse * scene.world_transform(bone).matrix()))
            })
            .collect()
    }

    /// Writes `mesh`'s skinning palette into `palette`, one transform per mesh bone in bone index
    /// order: each bone's root-space transform in `bones` times its inverse bind matrix. A bone
    /// missing from `bones` keeps the identity. Bone indices and inverse bind matrices belong to
    /// the mesh, since meshes sharing a skeleton can list its bones in different orders.
    pub fn skinning_palette(
        mesh: &Mesh,
        bones: &HashMap<String, Mat4>,
        palette: &mut Vec<BoneTransform>,
    ) {
        palette.clear();
        palette.resize(
            mesh.bones.len(),
            BoneTransform {
                transform: Mat4::identity().into(),
            },
        );
        for (name, bone) in &mesh.bones {
            if let (Some(transform), Some(slot)) = (bones.get(name), palette.get_mut(bone.index)) {
                slot.transform = (transform * bone.inverse_bind_transform).into();
            }
        }
    }
}
