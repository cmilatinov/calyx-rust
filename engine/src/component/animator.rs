use egui::Ui;
use glm::{Mat4, Quat, Vec3};
use nalgebra::Unit;
use serde::{Deserialize, Serialize};

use crate as engine;
use crate::assets::animation::{Animation, AnimationKeyFrames, QuatKeyFrame, VectorKeyFrame};
use crate::assets::mesh::BoneTransform;
use crate::core::{Ref, Time, TimeType};
use crate::scene::{GameObject, Scene};
use crate::{
    reflect::{Reflect, ReflectDefault},
    utils::{ReflectTypeUuidDynamic, TypeUuid},
};

use super::{Component, ComponentBone, ComponentSkinnedMesh, ReflectComponent};

#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "f24db81d-7054-40b8-8f3c-d9740c03948e"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Animator", update)]
#[serde(default)]
pub struct ComponentAnimator {
    pub animation: Option<Ref<Animation>>,
    pub time: TimeType,
}

impl Component for ComponentAnimator {
    fn update(&mut self, scene: &mut Scene, game_object: GameObject, _ui: &Ui) {
        let mut duration = None;
        let skinned_meshes = scene
            .get_children_with_component::<ComponentSkinnedMesh>(game_object)
            .collect::<Vec<_>>();
        if let Some(animation) = &self.animation {
            let animation = animation.read();
            duration = Some((animation.duration / animation.ticks_per_second) as TimeType);
            for skinned_mesh_go in skinned_meshes {
                let mut node_transforms: Vec<BoneTransform> = Default::default();
                let mut mesh_bone = None;
                if let Some(entry) = scene.entry(skinned_mesh_go) {
                    if let Ok(c_skinned_mesh) = entry.get_component::<ComponentSkinnedMesh>() {
                        if let Some(mesh) = &c_skinned_mesh.mesh {
                            if let Some(root_bone) = &c_skinned_mesh.root_bone {
                                mesh_bone = Some((mesh.clone(), *root_bone));
                            }
                        }
                    }
                }
                if let Some((mesh, root_bone)) = mesh_bone {
                    if let Some(root) = root_bone.game_object(scene) {
                        let mesh = mesh.read();
                        node_transforms.resize(
                            mesh.bones.len(),
                            BoneTransform {
                                transform: Mat4::identity().into(),
                            },
                        );
                        let transform = scene.get_transform(root);
                        self.traverse_bone_hierarchy(
                            scene,
                            root,
                            &animation,
                            &transform.inverse_matrix,
                            Mat4::identity(),
                            &mut node_transforms,
                        );
                    }
                }
                if let Some(mut entry) = scene.entry_mut(skinned_mesh_go) {
                    if let Ok(c_skinned_mesh) = entry.get_component_mut::<ComponentSkinnedMesh>() {
                        c_skinned_mesh.bone_transforms = node_transforms;
                    }
                }
            }
        }
        self.time += Time::delta_time();
        if let Some(duration) = duration {
            self.time %= duration;
        }
    }
}

impl ComponentAnimator {
    fn traverse_bone_hierarchy(
        &self,
        scene: &mut Scene,
        game_object: GameObject,
        animation: &Animation,
        global_inverse_transform: &Mat4,
        mut parent_transform: Mat4,
        node_transforms: &mut Vec<BoneTransform>,
    ) {
        let mut local_transform = scene.get_transform(game_object).matrix;
        if let Some(entry) = scene.entry(game_object) {
            if let Ok(c_bone) = entry.get_component::<ComponentBone>() {
                if let Some(keyframes) = animation.node_keyframes.get(&c_bone.name) {
                    local_transform = self.local_animation_bone_transform(
                        keyframes,
                        self.time * animation.ticks_per_second as f32,
                    );
                    node_transforms[c_bone.index] = BoneTransform {
                        transform: (global_inverse_transform
                            * parent_transform
                            * local_transform
                            * c_bone.offset_matrix)
                            .into(),
                    };
                }
            }
        }
        parent_transform = parent_transform * local_transform;
        scene.set_transform(game_object, local_transform);
        for child in scene.get_children(game_object).collect::<Vec<_>>() {
            self.traverse_bone_hierarchy(
                scene,
                child,
                animation,
                global_inverse_transform,
                parent_transform,
                node_transforms,
            );
        }
    }

    fn find_vector_keyframes<'a>(
        keyframes: &'a Vec<VectorKeyFrame>,
        time: f32,
    ) -> (&'a VectorKeyFrame, &'a VectorKeyFrame) {
        let mut prev = keyframes.first();
        let mut next = None;
        for keyframe in keyframes {
            if keyframe.time > time as f64 {
                next = Some(keyframe);
                break;
            }
            prev = Some(keyframe);
        }
        let prev = prev.unwrap();
        let next = next.unwrap_or(prev);
        (prev, next)
    }

    fn find_quat_keyframes<'a>(
        keyframes: &'a Vec<QuatKeyFrame>,
        time: f32,
    ) -> (&'a QuatKeyFrame, &'a QuatKeyFrame) {
        let mut prev = keyframes.first();
        let mut next = None;
        for keyframe in keyframes {
            if keyframe.time > time as f64 {
                next = Some(keyframe);
                break;
            }
            prev = Some(keyframe);
        }
        let prev = prev.unwrap();
        let next = next.unwrap_or(prev);
        (prev, next)
    }

    fn progression(prev_time: f32, next_time: f32, time: f32) -> f32 {
        let interval = next_time - prev_time;
        if interval.abs() < 0.00001 {
            return 0.0;
        }
        let time = time.clamp(prev_time, next_time);
        (time - prev_time) / interval
    }

    fn interpolate_vector(prev: &VectorKeyFrame, next: &VectorKeyFrame, time: f32) -> Vec3 {
        let progression = Self::progression(prev.time as f32, next.time as f32, time);
        prev.value.lerp(&next.value, progression)
    }

    fn interpolate_quat(prev: &QuatKeyFrame, next: &QuatKeyFrame, time: f32) -> Unit<Quat> {
        let progression = Self::progression(prev.time as f32, next.time as f32, time);
        prev.value.slerp(&next.value, progression)
    }

    fn local_animation_bone_transform(&self, keyframes: &AnimationKeyFrames, time: f32) -> Mat4 {
        let (prev, next) = Self::find_vector_keyframes(&keyframes.positions, time);
        let position = Self::interpolate_vector(prev, next, time);

        let (prev, next) = Self::find_quat_keyframes(&keyframes.rotations, time);
        let rotation = Self::interpolate_quat(prev, next, time);

        let (prev, next) = Self::find_vector_keyframes(&keyframes.scaling, time);
        let scaling = Self::interpolate_vector(prev, next, time);

        glm::translation(&position) * glm::quat_to_mat4(&rotation) * glm::scaling(&scaling)
    }
}
