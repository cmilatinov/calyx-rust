use super::{Component, ComponentBone, ComponentSkinnedMesh, ReflectComponent};
use crate as engine;
use crate::assets::animation::{Animation, AnimationKeyFrames, QuatKeyFrame, VectorKeyFrame};
use crate::assets::animation_graph::{
    AnimationCondition, AnimationGraph, AnimationParameterCondition, AnimationParameterValue,
    BoolCondition, FloatCondition, IntCondition,
};
use crate::assets::mesh::BoneTransform;
use crate::core::{Ref, Time, TimeType};
use crate::input::Input;
use crate::render::Gizmos;
use crate::scene::{GameObject, Scene};
use crate::{
    math,
    reflect::{Reflect, ReflectDefault},
    utils::{ReflectTypeUuidDynamic, TypeUuid},
};
use glm::{Mat4, Quat, Vec3, Vec4};
use nalgebra::Unit;
use petgraph::prelude::{EdgeIndex, EdgeRef, NodeIndex};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Clone, Copy)]
struct AnimatorTransition {
    transition: EdgeIndex,
    source_time_start: f32,
    time: f32,
    has_exit_time: bool,
    exit_time: f32,
    duration: f32,
}

#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "f24db81d-7054-40b8-8f3c-d9740c03948e"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Animator", update)]
#[serde(default)]
pub struct ComponentAnimator {
    pub animation: Option<Ref<Animation>>,
    pub time: TimeType,
    pub draw_debug_skeleton: bool,
    pub animation_graph: Option<Ref<AnimationGraph>>,
    #[reflect_skip]
    #[serde(skip)]
    current_state: Option<NodeIndex>,
    #[reflect_skip]
    #[serde(skip)]
    current_transition: Option<AnimatorTransition>,
    #[reflect_skip]
    #[serde(skip)]
    parameters: HashMap<Uuid, AnimationParameterValue>,
    #[reflect_skip]
    #[serde(skip)]
    node_transforms: Vec<BoneTransform>,
}

impl Component for ComponentAnimator {
    fn reset(&mut self, scene: &mut Scene, game_object: GameObject) {
        self.apply_animation_pose(scene, game_object);
    }

    fn start(&mut self, scene: &mut Scene, game_object: GameObject) {
        let Some(graph_ref) = self.animation_graph.clone() else {
            return;
        };
        let graph = graph_ref.read();
        self.current_state = graph
            .start_node
            .and_then(|id| graph.node_indices().find(|n| graph[*n].id == id));
    }

    fn update(&mut self, scene: &mut Scene, game_object: GameObject, _input: &Input) {
        self.step_fsm();
        let duration = self.apply_animation_pose(scene, game_object);
        self.update_time(duration);
    }

    fn draw_gizmos(&self, scene: &Scene, game_object: GameObject, gizmos: &mut Gizmos) {
        if self.draw_debug_skeleton {
            gizmos.set_color(&Vec4::new(1.0, 1.0, 0.0, 1.0));
            let Some(root) = scene
                .get_descendants_with_component::<ComponentBone>(game_object)
                .next()
            else {
                return;
            };
            let transform = scene.get_world_transform(root);
            for child in scene.get_children(root) {
                self.draw_bones(scene, child, gizmos, transform.position);
            }
        }
    }
}

impl ComponentAnimator {
    fn step_fsm(&mut self) {
        let new_transition;
        let Some(graph_ref) = self.animation_graph.clone() else {
            return;
        };
        let graph = graph_ref.read();
        {
            if let Some(transition) = self.current_transition {
                if transition.time > transition.duration {
                    self.end_transition(&graph);
                }
                return;
            }
            let Some(mut transitions) = self.current_state.map(|n| graph.edges(n)) else {
                return;
            };
            let Some(transition) = transitions.find(|transition| {
                transition
                    .weight()
                    .conditions
                    .iter()
                    .all(|cond| Self::condition_satisfied(&self.parameters, cond))
            }) else {
                return;
            };
            new_transition = transition.id();
        }
        self.begin_transition(new_transition, &graph);
    }

    fn begin_transition(&mut self, transition: EdgeIndex, graph: &AnimationGraph) {
        if let Some((duration, has_exit_time, exit_time)) =
            graph.edge_weight(transition).map(|transition| {
                (
                    transition.duration,
                    transition.has_exit_time,
                    transition.exit_time,
                )
            })
        {
            self.current_transition = Some(AnimatorTransition {
                transition,
                source_time_start: self.time,
                time: 0.0,
                duration,
                has_exit_time,
                exit_time,
            });
        }
    }

    fn end_transition(&mut self, graph: &AnimationGraph) {
        if let Some(transition) = self.current_transition.take() {
            if let Some((source, target)) = graph.edge_endpoints(transition.transition) {
                // let source_duration
                self.current_state = Some(target);
            }
        }
        self.current_transition = None;
    }

    fn condition_satisfied(
        parameters: &HashMap<Uuid, AnimationParameterValue>,
        condition: &AnimationParameterCondition,
    ) -> bool {
        parameters
            .get(&condition.parameter)
            .map(|p| match (p, &condition.condition) {
                (_, AnimationCondition::None) => false,
                (
                    AnimationParameterValue::Float(param),
                    AnimationCondition::Float(FloatCondition::Less(value)),
                ) => *param < *value,
                (
                    AnimationParameterValue::Float(param),
                    AnimationCondition::Float(FloatCondition::Greater(value)),
                ) => *param > *value,
                (
                    AnimationParameterValue::Int(param),
                    AnimationCondition::Int(IntCondition::Less(value)),
                ) => *param < *value,
                (
                    AnimationParameterValue::Int(param),
                    AnimationCondition::Int(IntCondition::Greater(value)),
                ) => *param > *value,
                (
                    AnimationParameterValue::Int(param),
                    AnimationCondition::Int(IntCondition::Equal(value)),
                ) => *param == *value,
                (
                    AnimationParameterValue::Int(param),
                    AnimationCondition::Int(IntCondition::NotEqual(value)),
                ) => *param != *value,
                (
                    AnimationParameterValue::Bool(param),
                    AnimationCondition::Bool(BoolCondition::True),
                ) => *param,
                (
                    AnimationParameterValue::Bool(param),
                    AnimationCondition::Bool(BoolCondition::False),
                ) => !*param,
                (AnimationParameterValue::Trigger, AnimationCondition::Trigger) => true,
                _ => false,
            })
            .unwrap_or(false)
    }

    fn apply_animation_pose(
        &mut self,
        scene: &mut Scene,
        game_object: GameObject,
    ) -> Option<TimeType> {
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
        duration
    }

    fn update_time(&mut self, animation_duration: Option<TimeType>) {
        let delta_time = Time::delta_time();
        self.time += delta_time;
        if let Some(duration) = animation_duration {
            self.time %= duration;
        }
        if let Some(transition) = &mut self.current_transition {
            transition.time += delta_time;
        }
    }

    fn draw_bones(
        &self,
        scene: &Scene,
        game_object: GameObject,
        gizmos: &mut Gizmos,
        parent_position: Vec3,
    ) {
        let transform = scene.get_world_transform(game_object);
        let is_bone = scene
            .entry(game_object)
            .map(|entry| entry.get_component::<ComponentBone>().is_ok())
            .unwrap_or(false);
        if is_bone {
            gizmos.line(&parent_position, &transform.position);
        }
        for child in scene.get_children(game_object) {
            self.draw_bones(
                scene,
                child,
                gizmos,
                if is_bone {
                    transform.position
                } else {
                    parent_position
                },
            );
        }
    }

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
