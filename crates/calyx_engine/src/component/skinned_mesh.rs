use super::{Component, ComponentBone, ReflectComponent};
use crate as calyx_engine;
use crate::assets::material::Material;
use crate::assets::mesh::{BoneTransform, Mesh};
use crate::assets::AssetRef;
use crate::math::Transform;
use crate::reflect::{Reflect, ReflectDefault};
use crate::scene::{GameObject, GameObjectRef, Scene};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra_glm::Mat4;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Mesh renderer component driven by a bone hierarchy.
#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
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
    /// Whether the mesh draws into the shadow maps of shadowed lights, in
    /// its posed shape.
    pub cast_shadows: bool,
    /// Runtime bone matrices uploaded to the renderer.
    #[reflect_skip]
    #[serde(skip)]
    pub bone_transforms: Vec<BoneTransform>,
}

impl Default for ComponentSkinnedMesh {
    fn default() -> Self {
        Self {
            material: Default::default(),
            mesh: Default::default(),
            root_bone: Default::default(),
            cast_shadows: true,
            bone_transforms: Default::default(),
        }
    }
}

impl Component for ComponentSkinnedMesh {
    /// The scene renderer skips a disabled skinned mesh: it is neither drawn
    /// nor picked.
    fn can_disable(&self) -> bool {
        true
    }
}

impl ComponentSkinnedMesh {
    /// World transform that places this mesh on `game_object`. The animator skins its vertices
    /// into the space of its root bone, so the root bone's world transform places them; the mesh
    /// object's own world transform would apply every node between the root bone and the mesh a
    /// second time. A mesh whose root bone is not in the scene keeps its own transform.
    pub fn world_transform(&self, scene: &Scene, game_object: GameObject) -> Transform {
        let placement = self.root_bone.game_object(scene).unwrap_or(game_object);
        scene.world_transform(placement)
    }

    /// Root-space transform of each bone object under `root`, by bone name, as the scene holds
    /// them. Reads the scene only, so it poses a skinned mesh without running its animator.
    ///
    /// Local matrices are multiplied down from `root`, as the animator's traversal does, rather
    /// than read back from world transforms: a world transform is cached decomposed, which drops
    /// the shear a non-uniformly scaled ancestor gives a rotated bone.
    pub fn bone_poses(scene: &Scene, root: GameObject) -> HashMap<String, Mat4> {
        let mut bones = HashMap::new();
        let mut stack = vec![(root, Mat4::identity())];
        while let Some((game_object, transform)) = stack.pop() {
            if let Some(name) =
                scene.read_component::<ComponentBone, _, _>(game_object, |c| c.name.clone())
            {
                bones.insert(name, transform);
            }
            for child in scene.children(game_object) {
                stack.push((child, transform * scene.transform(child).matrix()));
            }
        }
        bones
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
