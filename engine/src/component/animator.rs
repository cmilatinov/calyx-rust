use super::{
    Component, ComponentBone, ComponentEventContext, ComponentReset, ComponentSkinnedMesh,
    ComponentUpdate, ReflectComponent, ReflectComponentReset, ReflectComponentUpdate,
};
use crate as engine;
use crate::assets::animation::{AnimationKeyFrames, QuatKeyFrame, VectorKeyFrame};
use crate::assets::animation_graph::{
    AnimationCondition, AnimationGraph, AnimationMotion, AnimationParameterCondition,
    AnimationParameterValue, BoolCondition, FloatCondition, IntCondition,
};
use crate::assets::mesh::BoneTransform;
use crate::assets::AssetRef;
use crate::context::ReadOnlyRegistryContext;
use crate::core::{Time, TimeType};
use crate::input::Input;
use crate::math::Transform;
use crate::render::Gizmos;
use crate::resource::ResourceMap;
use crate::scene::{GameObject, Scene};
use crate::{
    reflect::{Reflect, ReflectDefault},
    utils::{ReflectTypeUuidDynamic, TypeUuid},
};
use lerp::Lerp;
use nalgebra::Unit;
use nalgebra_glm::{Mat4, Quat, Vec3, Vec4};
use petgraph::prelude::{EdgeIndex, EdgeRef, NodeIndex};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Clone, Copy)]
#[repr(C)]
struct AnimatorTransition {
    transition: EdgeIndex,
    source_time_start: f32,
    time: f32,
    has_exit_time: bool,
    exit_time: f32,
    duration: f32,
}

#[derive(Default, Clone)]
#[repr(C)]
struct AnimatorPose {
    bone_transforms: Vec<BoneTransform>,
}

/// Read-only snapshot of animator state needed for pose computation.
/// Avoids cloning the entire ComponentAnimator (which includes the pose Vec).
struct AnimatorSnapshot {
    time: TimeType,
    current_state: Option<NodeIndex>,
    parameters: HashMap<Uuid, AnimationParameterValue>,
    animation_graph: AssetRef<AnimationGraph>,
}

impl AnimatorSnapshot {
    fn from_component(c: &ComponentAnimator) -> Self {
        Self {
            time: c.time,
            current_state: c.current_state,
            parameters: c.parameters.clone(),
            animation_graph: c.animation_graph.clone(),
        }
    }
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[repr(transparent)]
pub struct AnimationParameters(pub HashMap<Uuid, AnimationParameterValue>);

impl Lerp<f32> for AnimationParameters {
    fn lerp(mut self, Self(other): Self, t: f32) -> Self {
        for (id, other) in other {
            if let Some(current) = self.0.get_mut(&id) {
                current.lerp_to(other, t);
            } else {
                self.0.insert(id, other);
            }
        }
        self
    }
}

#[derive(Default, Clone, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "f24db81d-7054-40b8-8f3c-d9740c03948e"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate, ComponentReset)]
#[reflect_attr(name = "Animator")]
#[serde(default)]
#[repr(C)]
pub struct ComponentAnimator {
    pub time: TimeType,
    pub draw_debug_skeleton: bool,
    pub animation_graph: AssetRef<AnimationGraph>,
    #[reflect_skip]
    #[serde(skip)]
    current_state: Option<NodeIndex>,
    #[reflect_skip]
    #[serde(skip)]
    current_transition: Option<AnimatorTransition>,
    #[reflect_skip]
    #[serde(skip)]
    current_pose: AnimatorPose,
    #[reflect_skip]
    #[serde(skip)]
    pub parameters: HashMap<Uuid, AnimationParameterValue>,
}

impl Component for ComponentAnimator {
    fn draw_gizmos(&self, scene: &Scene, game_object: GameObject, gizmos: &mut Gizmos) {
        if self.draw_debug_skeleton {
            gizmos.set_color(&Vec4::new(1.0, 1.0, 0.0, 1.0));
            let Some(root) = scene
                .descendants_with::<ComponentBone>(game_object)
                .next()
            else {
                return;
            };
            let transform = scene.world_transform(root);
            for child in scene.children(root) {
                Self::draw_bones(scene, child, gizmos, transform.position);
            }
        }
    }
}

impl ComponentReset for ComponentAnimator {
    fn reset(
        &self,
        ComponentEventContext {
            registries: assets,
            scene,
            game_object,
        }: ComponentEventContext,
    ) {
        let mut pose = AnimatorPose::default();
        Self::compute_animation_pose(assets, scene, game_object, &mut pose);
        scene.write_component::<ComponentAnimator, _>(game_object, |c| {
            c.current_pose = pose;
        });
    }
}

impl ComponentUpdate for ComponentAnimator {
    fn update(
        &self,
        ComponentEventContext {
            registries: assets,
            scene,
            game_object,
        }: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        // init and step_fsm only touch animator fields — no Scene needed.
        scene.write_component::<ComponentAnimator, _>(game_object, |c| {
            c.init(assets);
            c.step_fsm(assets);
        });

        // Compute the animation pose using a temporary buffer.
        // This reads animator fields from the ECS and writes transforms to the Scene,
        // but never holds &mut ComponentAnimator and &mut Scene simultaneously.
        let mut pose = AnimatorPose::default();
        Self::compute_animation_pose(assets, scene, game_object, &mut pose);

        // Write back the pose and advance time.
        let time = resources.time();
        scene.write_component::<ComponentAnimator, _>(game_object, |c| {
            c.current_pose = pose;
            c.update_time(time);
        });
    }
}

impl ComponentAnimator {
    pub fn parameters(&self) -> &HashMap<Uuid, AnimationParameterValue> {
        &self.parameters
    }

    pub fn parameters_mut(&mut self) -> &mut HashMap<Uuid, AnimationParameterValue> {
        &mut self.parameters
    }
}

impl ComponentAnimator {
    pub fn set_parameter(
        &mut self,
        assets: &ReadOnlyRegistryContext,
        name: &str,
        value: AnimationParameterValue,
    ) -> bool {
        self.set_parameter_with(assets, name, |_| value)
    }

    pub fn set_parameter_with<
        F: FnOnce(Option<AnimationParameterValue>) -> AnimationParameterValue,
    >(
        &mut self,
        assets: &ReadOnlyRegistryContext,
        name: &str,
        setter: F,
    ) -> bool {
        let Some(animation_graph) = self.animation_graph.get_ref(assets) else {
            return false;
        };
        let Some(parameter_id) = animation_graph.read().parameters.iter().find_map(|p| {
            if p.name.as_str() == name {
                Some(p.id)
            } else {
                None
            }
        }) else {
            return false;
        };
        let current_value = self.parameters.get(&parameter_id).cloned();
        self.parameters.insert(parameter_id, setter(current_value));
        true
    }

    fn init(&mut self, assets: &ReadOnlyRegistryContext) {
        if !self.parameters.is_empty() {
            return;
        }
        let Some(graph_ref) = self.animation_graph.get_ref(assets) else {
            return;
        };
        let graph = graph_ref.read();
        for param in graph.parameters.iter() {
            self.parameters.insert(param.id, param.value);
        }
        self.current_state = graph
            .start_node
            .and_then(|id| graph.node_indices().find(|n| graph[*n].id == id));
    }

    fn step_fsm(&mut self, assets: &ReadOnlyRegistryContext) {
        let new_transition;
        let Some(graph_ref) = self.animation_graph.get_ref(assets) else {
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
            if let Some((_source, target)) = graph.edge_endpoints(transition.transition) {
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

    /// Static method: computes animation pose without holding &mut ComponentAnimator.
    /// Reads animator state from the ECS via a lightweight snapshot, writes bone
    /// transforms to the Scene, and returns the computed pose.
    fn compute_animation_pose(
        assets: &ReadOnlyRegistryContext,
        scene: &mut Scene,
        game_object: GameObject,
        pose: &mut AnimatorPose,
    ) {
        let Some(snapshot) =
            scene.read_component::<ComponentAnimator, _, _>(game_object, |c| {
                AnimatorSnapshot::from_component(c)
            })
        else {
            return;
        };
        let skinned_meshes = scene
            .descendants_with::<ComponentSkinnedMesh>(game_object)
            .collect::<Vec<_>>();
        let Some(animation_graph) = snapshot.animation_graph.get_ref(assets) else {
            return;
        };
        let animation_graph = animation_graph.read();
        for skinned_mesh_go in skinned_meshes {
            let Some(entry) = scene.entry(skinned_mesh_go) else {
                continue;
            };
            let Ok(c_skinned_mesh) = entry.get_component::<ComponentSkinnedMesh>() else {
                continue;
            };
            let Some(root) = c_skinned_mesh.root_bone.game_object(scene) else {
                continue;
            };
            let Some(mesh_ref) = c_skinned_mesh.mesh.get_ref(assets) else {
                continue;
            };
            let mesh = mesh_ref.read();
            pose.bone_transforms.resize(
                mesh.bones.len(),
                BoneTransform {
                    transform: Mat4::identity().into(),
                },
            );
            let transform = scene.transform(root);
            Self::traverse_bone_hierarchy_static(
                &snapshot,
                assets,
                scene,
                root,
                &animation_graph,
                &transform.inverse_matrix(),
                Mat4::identity(),
                pose,
            );
            let Some(mut entry) = scene.entry_mut(skinned_mesh_go) else {
                continue;
            };
            let Ok(c_skinned_mesh) = entry.get_component_mut::<ComponentSkinnedMesh>() else {
                continue;
            };
            c_skinned_mesh.bone_transforms.clear();
            c_skinned_mesh
                .bone_transforms
                .extend(pose.bone_transforms.drain(0..));
        }
    }

    fn update_time(&mut self, time: &Time) {
        let delta_time = time.delta_time();
        self.time += delta_time;
        if let Some(transition) = &mut self.current_transition {
            transition.time += delta_time;
        }
    }

    fn draw_bones(
        scene: &Scene,
        game_object: GameObject,
        gizmos: &mut Gizmos,
        parent_position: Vec3,
    ) {
        let transform = scene.world_transform(game_object);
        let is_bone = scene
            .entry(game_object)
            .map(|entry| entry.get_component::<ComponentBone>().is_ok())
            .unwrap_or(false);
        if is_bone {
            gizmos.line(&parent_position, &transform.position);
        }
        for child in scene.children(game_object) {
            Self::draw_bones(
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

    fn traverse_bone_hierarchy_static(
        snapshot: &AnimatorSnapshot,
        assets: &ReadOnlyRegistryContext,
        scene: &mut Scene,
        game_object: GameObject,
        animation_graph: &AnimationGraph,
        global_inverse_transform: &Mat4,
        mut parent_transform: Mat4,
        pose: &mut AnimatorPose,
    ) {
        let mut local_transform = scene.transform(game_object).matrix();
        'calc_bone_transform: {
            let Some(entry) = scene.entry(game_object) else {
                break 'calc_bone_transform;
            };
            let Ok(c_bone) = entry.get_component::<ComponentBone>() else {
                break 'calc_bone_transform;
            };
            let Some(node) = snapshot
                .current_state
                .and_then(|nid| animation_graph.graph.node_weight(nid))
            else {
                break 'calc_bone_transform;
            };
            local_transform = Self::motion_local_bone_transform_static(
                &snapshot.parameters,
                assets,
                &node.motion,
                &c_bone.name,
                snapshot.time,
            )
            .matrix();
            pose.bone_transforms[c_bone.index] = BoneTransform {
                transform: (global_inverse_transform
                    * parent_transform
                    * local_transform
                    * c_bone.offset_matrix)
                    .into(),
            };
        }
        parent_transform *= local_transform;
        scene.set_transform(game_object, &local_transform);
        let mut walker = scene.children_walker(game_object);
        while let Some(child) = walker.next(scene) {
            Self::traverse_bone_hierarchy_static(
                snapshot,
                assets,
                scene,
                child,
                animation_graph,
                global_inverse_transform,
                parent_transform,
                pose,
            );
        }
    }

    fn find_keyframes<T, F: FnMut(&T) -> TimeType>(
        keyframes: &[T],
        ticks: TimeType,
        mut accessor: F,
    ) -> (&T, &T) {
        let index = keyframes
            .binary_search_by(|k| accessor(k).partial_cmp(&ticks).unwrap_or(Ordering::Less))
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

    fn interpolate_vector(prev: &VectorKeyFrame, next: &VectorKeyFrame, ticks: f32) -> Vec3 {
        let progression = Self::progression(prev.time as f32, next.time as f32, ticks);
        prev.value.lerp(next.value, progression)
    }

    fn interpolate_quat(prev: &QuatKeyFrame, next: &QuatKeyFrame, ticks: f32) -> Unit<Quat> {
        let progression = Self::progression(prev.time as f32, next.time as f32, ticks);
        prev.value.slerp(&next.value, progression)
    }

    fn motion_local_bone_transform_static(
        parameters: &HashMap<Uuid, AnimationParameterValue>,
        assets: &ReadOnlyRegistryContext,
        motion: &AnimationMotion,
        bone_name: &str,
        time: f32,
    ) -> Transform {
        match motion {
            AnimationMotion::AnimationClip(clip) => {
                let Some(animation) = clip.animation.get_ref(assets) else {
                    return Default::default();
                };
                let animation = animation.read();
                let Some(keyframes) = animation.node_keyframes.get(bone_name) else {
                    return Default::default();
                };
                Self::animation_local_bone_transform(
                    keyframes,
                    (clip.speed * time * animation.ticks_per_second as f32)
                        % animation.duration as f32,
                )
            }
            AnimationMotion::BlendTree1D(tree) => {
                Transform::nlerp(tree.nearest_neighbors(2, parameters).into_iter().map(
                    |(weight, neighbor)| {
                        (
                            weight,
                            Self::motion_local_bone_transform_static(
                                parameters,
                                assets,
                                &neighbor.motion,
                                bone_name,
                                time,
                            ),
                        )
                    },
                ))
            }
            AnimationMotion::BlendTree2D(tree) => {
                Transform::nlerp(tree.nearest_neighbors(4, parameters).into_iter().map(
                    |(weight, neighbor)| {
                        (
                            weight,
                            Self::motion_local_bone_transform_static(
                                parameters,
                                assets,
                                &neighbor.motion,
                                bone_name,
                                time,
                            ),
                        )
                    },
                ))
            }
        }
    }

    fn animation_local_bone_transform(keyframes: &AnimationKeyFrames, ticks: f32) -> Transform {
        let (prev, next) = Self::find_keyframes(&keyframes.positions, ticks, |k| k.time as f32);
        let position = Self::interpolate_vector(prev, next, ticks);

        let (prev, next) = Self::find_keyframes(&keyframes.rotations, ticks, |k| k.time as f32);
        let rotation = Self::interpolate_quat(prev, next, ticks);

        let (prev, next) = Self::find_keyframes(&keyframes.scaling, ticks, |k| k.time as f32);
        let scale = Self::interpolate_vector(prev, next, ticks);

        Transform {
            position,
            rotation,
            scale,
        }
    }
}
