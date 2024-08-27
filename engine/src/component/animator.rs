use super::{Component, ComponentBone, ComponentSkinnedMesh, ReflectComponent};
use crate as engine;
use crate::assets::animation::{Animation, AnimationKeyFrames, QuatKeyFrame, VectorKeyFrame};
use crate::assets::mesh::BoneTransform;
use crate::core::{Ref, Time, TimeType};
use crate::input::Input;
use crate::scene::{GameObject, Scene};
use crate::{
    math,
    reflect::{Reflect, ReflectDefault},
    utils::{ReflectTypeUuidDynamic, TypeUuid},
};
use glm::{Mat4, Quat, Vec3};
use nalgebra::Unit;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "f24db81d-7054-40b8-8f3c-d9740c03948e"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Animator", update)]
#[serde(default)]
pub struct ComponentAnimator {
    pub animation: Option<Ref<Animation>>,
    pub time: TimeType,
    #[reflect_skip]
    #[serde(skip)]
    node_transforms: Vec<BoneTransform>,
}

impl Component for ComponentAnimator {
    fn update(&mut self, scene: &mut Scene, game_object: GameObject, _input: &Input) {
        let mut duration = None;
        let skinned_meshes = scene
            .get_descendants_with_component::<ComponentSkinnedMesh>(game_object)
            .collect::<Vec<_>>();
        if let Some(animation) = self.animation.clone() {
            let animation = animation.read();
            duration = Some((animation.duration / animation.ticks_per_second) as TimeType);
            for skinned_mesh_go in skinned_meshes {
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
                        self.node_transforms.resize(
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
                        );
                    }
                }
                if let Some(mut entry) = scene.entry_mut(skinned_mesh_go) {
                    if let Ok(c_skinned_mesh) = entry.get_component_mut::<ComponentSkinnedMesh>() {
                        c_skinned_mesh.bone_transforms.clear();
                        c_skinned_mesh
                            .bone_transforms
                            .extend(self.node_transforms.drain(0..));
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
        &mut self,
        scene: &mut Scene,
        game_object: GameObject,
        animation: &Animation,
        global_inverse_transform: &Mat4,
        mut parent_transform: Mat4,
    ) {
        let mut local_transform = scene.get_transform(game_object).matrix;
        if let Some(entry) = scene.entry(game_object) {
            if let Ok(c_bone) = entry.get_component::<ComponentBone>() {
                if let Some(keyframes) = animation.node_keyframes.get(&c_bone.name) {
                    local_transform = self.local_animation_bone_transform(
                        keyframes,
                        self.time * animation.ticks_per_second as f32,
                    );
                    self.node_transforms[c_bone.index] = BoneTransform {
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
        let mut walker = scene.get_children_walker(game_object);
        while let Some(child) = walker.next(scene) {
            self.traverse_bone_hierarchy(
                scene,
                child,
                animation,
                global_inverse_transform,
                parent_transform,
            );
        }
    }

    fn find_keyframes<T, F: FnMut(&T) -> TimeType>(
        keyframes: &Vec<T>,
        time: TimeType,
        mut accessor: F,
    ) -> (&T, &T) {
        let index = keyframes
            .binary_search_by(|k| accessor(k).partial_cmp(&time).unwrap_or(Ordering::Less))
            .unwrap_or_else(|index| index) as isize;
        let max_index = (keyframes.len() - 1) as isize;
        let prev = (index - 1).clamp(0, max_index) as usize;
        let next = index.clamp(0, max_index) as usize;
        (&keyframes[prev], &keyframes[next])
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
        let (prev, next) = Self::find_keyframes(&keyframes.positions, time, |k| k.time as f32);
        let position = Self::interpolate_vector(prev, next, time);

        let (prev, next) = Self::find_keyframes(&keyframes.rotations, time, |k| k.time as f32);
        let rotation = Self::interpolate_quat(prev, next, time);

        let (prev, next) = Self::find_keyframes(&keyframes.scaling, time, |k| k.time as f32);
        let scaling = Self::interpolate_vector(prev, next, time);

        math::compose_transform(&position, &rotation, &scaling)
    }
}
