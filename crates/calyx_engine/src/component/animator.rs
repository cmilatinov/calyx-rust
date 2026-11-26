use super::{
    Component, ComponentBone, ComponentEventContext, ComponentRef, ComponentReset,
    ComponentSkinnedMesh, ComponentUpdate, ReflectComponent, ReflectComponentReset,
    ReflectComponentUpdate,
};
use crate as calyx_engine;
use crate::assets::animation::{AnimationKeyFrames, QuatKeyFrame, VectorKeyFrame};
use crate::assets::animation_graph::{
    AnimationCondition, AnimationGraph, AnimationMotion, AnimationParameterCondition,
    AnimationParameterValue, AnimationTransition, BoolCondition, FloatCondition, IntCondition,
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
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Clone, Copy)]
struct AnimatorTransition {
    transition: EdgeIndex,
    time: f32,
    duration: f32,
}

#[derive(Default, Clone)]
struct AnimatorPose {
    bone_transforms: Vec<BoneTransform>,
}

/// Read-only snapshot of animator state needed for pose computation.
/// Avoids cloning the entire ComponentAnimator (which includes the pose Vec).
struct AnimatorSnapshot {
    state_time: TimeType,
    current_state: Option<NodeIndex>,
    parameters: HashMap<Uuid, AnimationParameterValue>,
    animation_graph: AssetRef<AnimationGraph>,
}

impl AnimatorSnapshot {
    fn from_component(c: &ComponentAnimator) -> Self {
        Self {
            state_time: c.state_time,
            current_state: c.current_state,
            parameters: c.parameters.clone(),
            animation_graph: c.animation_graph.clone(),
        }
    }
}

/// Serializable animation parameter map stored on [`ComponentAnimator`].
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

/// Animation state-machine component that drives skinned meshes.
#[derive(Default, Clone, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "f24db81d-7054-40b8-8f3c-d9740c03948e"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate, ComponentReset)]
#[reflect_attr(name = "Animator")]
#[serde(default)]
pub struct ComponentAnimator {
    /// Total time in seconds the animator has played. Clips play from the time spent in the
    /// current state instead, so a state entered restarts its clip.
    pub time: TimeType,
    /// Whether to draw the bone hierarchy as debug gizmos.
    pub draw_debug_skeleton: bool,
    /// Animation graph asset driving this animator.
    pub animation_graph: AssetRef<AnimationGraph>,
    #[reflect_skip]
    #[serde(skip)]
    current_state: Option<NodeIndex>,
    /// Seconds spent in the current state, from 0 when it was entered.
    #[reflect_skip]
    #[serde(skip)]
    state_time: TimeType,
    /// Normalized time the state machine last checked the current state's transitions at, so
    /// an exit time a frame steps over is still met.
    #[reflect_skip]
    #[serde(skip)]
    checked_normalized_time: Option<f32>,
    #[reflect_skip]
    #[serde(skip)]
    current_transition: Option<AnimatorTransition>,
    #[reflect_skip]
    #[serde(skip)]
    current_pose: AnimatorPose,
    /// Runtime parameter values keyed by parameter UUID.
    #[reflect_skip]
    #[serde(skip)]
    pub parameters: HashMap<Uuid, AnimationParameterValue>,
}

impl Component for ComponentAnimator {
    /// A disabled animator skips its update hook, so it neither advances nor
    /// writes a pose: its skinned meshes keep the last pose it wrote, or the
    /// rest pose when it never wrote one.
    fn can_disable(&self) -> bool {
        true
    }

    fn has_gizmos(&self) -> bool {
        true
    }

    fn draw_gizmos(&self, scene: &Scene, game_object: GameObject, gizmos: &mut Gizmos) {
        if self.draw_debug_skeleton {
            gizmos.set_color(&Vec4::new(1.0, 1.0, 0.0, 1.0));
            let Some(root) = scene.descendants_with::<ComponentBone>(game_object).next() else {
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
        this: ComponentRef<Self>,
        ComponentEventContext {
            registries: assets,
            scene,
            game_object,
        }: ComponentEventContext,
    ) {
        let mut pose = AnimatorPose::default();
        Self::compute_animation_pose(assets, scene, game_object, &mut pose);
        this.write(scene, |c| {
            c.current_pose = pose;
        });
    }
}

impl ComponentUpdate for ComponentAnimator {
    fn update(
        this: ComponentRef<Self>,
        ComponentEventContext {
            registries: assets,
            scene,
            game_object,
        }: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        // init and step_fsm only touch animator fields — no Scene needed.
        this.write(scene, |c| {
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
        this.write(scene, |c| {
            c.current_pose = pose;
            c.update_time(time);
        });
    }
}

impl ComponentAnimator {
    /// Returns the animator parameter map.
    pub fn parameters(&self) -> &HashMap<Uuid, AnimationParameterValue> {
        &self.parameters
    }

    /// Returns the mutable animator parameter map.
    pub fn parameters_mut(&mut self) -> &mut HashMap<Uuid, AnimationParameterValue> {
        &mut self.parameters
    }
}

impl ComponentAnimator {
    /// Sets a named animation parameter to `value`.
    pub fn set_parameter(
        &mut self,
        assets: &ReadOnlyRegistryContext,
        name: &str,
        value: AnimationParameterValue,
    ) -> bool {
        self.set_parameter_with(assets, name, |_| value)
    }

    /// Updates a named animation parameter using `setter`.
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
        // A graph without parameters leaves the map empty, so the start state also marks that
        // init has run.
        if !self.parameters.is_empty() || self.current_state.is_some() {
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
        self.state_time = 0.0;
        self.checked_normalized_time = None;
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
            let Some(state) = self.current_state else {
                return;
            };
            let normalized_time = graph
                .node_weight(state)
                .and_then(|node| Self::motion_length(&self.parameters, assets, &node.motion))
                .map(|length| self.state_time / length);
            let checked_normalized_time = self.checked_normalized_time;
            self.checked_normalized_time = normalized_time;
            let Some(transition) = graph.edges(state).find(|transition| {
                let transition = transition.weight();
                Self::exit_time_reached(transition, checked_normalized_time, normalized_time)
                    && transition
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
        if let Some(duration) = graph
            .edge_weight(transition)
            .map(|transition| transition.duration)
        {
            self.current_transition = Some(AnimatorTransition {
                transition,
                time: 0.0,
                duration,
            });
        }
    }

    fn end_transition(&mut self, graph: &AnimationGraph) {
        if let Some(transition) = self.current_transition.take() {
            if let Some((_source, target)) = graph.edge_endpoints(transition.transition) {
                self.current_state = Some(target);
                self.state_time = 0.0;
                self.checked_normalized_time = None;
            }
        }
        self.current_transition = None;
    }

    /// Whether `transition` may start at `normalized_time`, the time in its source state over
    /// the state's length. As in Unity, an exit time below 1 is met at that fraction of every
    /// loop, and one of 1 or more is met once that many loops have played and then at the same
    /// fraction of each later loop. An exit point that a frame steps over since
    /// `checked_normalized_time`, the normalized time last checked in this state, is met too,
    /// even when the frame lands in the next loop before the exit fraction. A state without a
    /// length, such as one whose clip is missing, does not hold its transitions back.
    fn exit_time_reached(
        transition: &AnimationTransition,
        checked_normalized_time: Option<f32>,
        normalized_time: Option<f32>,
    ) -> bool {
        if !transition.has_exit_time {
            return true;
        }
        let Some(normalized_time) = normalized_time else {
            return true;
        };
        let exit_time = transition.exit_time;
        if normalized_time >= exit_time && normalized_time.fract() >= exit_time.fract() {
            return true;
        }
        // The exit points are `exit_time + k` for every whole k >= 0. Find the first one after
        // the last check and see whether this frame has reached it.
        checked_normalized_time.is_some_and(|checked| {
            let next_exit_point = if checked < exit_time {
                exit_time
            } else {
                exit_time + (checked - exit_time).floor() + 1.0
            };
            next_exit_point <= normalized_time
        })
    }

    /// Seconds one loop of `motion` lasts. A blend tree lasts the average of its clips' lengths,
    /// weighted by their current blend weights, as in Unity.
    fn motion_length(
        parameters: &HashMap<Uuid, AnimationParameterValue>,
        assets: &ReadOnlyRegistryContext,
        motion: &AnimationMotion,
    ) -> Option<TimeType> {
        match motion {
            AnimationMotion::AnimationClip(clip) => {
                let animation = clip.animation.get_ref(assets)?;
                let animation = animation.read();
                let length = animation.duration as f32
                    / (clip.speed.abs() * animation.ticks_per_second as f32);
                (length.is_finite() && length > 0.0).then_some(length)
            }
            AnimationMotion::BlendTree1D(tree) => Self::blend_length(
                parameters,
                assets,
                tree.nearest_neighbors(2, parameters)
                    .into_iter()
                    .map(|(weight, neighbor)| (weight, &neighbor.motion)),
            ),
            AnimationMotion::BlendTree2D(tree) => Self::blend_length(
                parameters,
                assets,
                tree.nearest_neighbors(4, parameters)
                    .into_iter()
                    .map(|(weight, neighbor)| (weight, &neighbor.motion)),
            ),
        }
    }

    fn blend_length<'a>(
        parameters: &HashMap<Uuid, AnimationParameterValue>,
        assets: &ReadOnlyRegistryContext,
        motions: impl IntoIterator<Item = (f32, &'a AnimationMotion)>,
    ) -> Option<TimeType> {
        let (weighted, total) = motions
            .into_iter()
            .filter_map(|(weight, motion)| {
                Self::motion_length(parameters, assets, motion).map(|length| (weight, length))
            })
            .fold((0.0, 0.0), |(weighted, total), (weight, length)| {
                (weighted + weight * length, total + weight)
            });
        let length = weighted / total;
        (length.is_finite() && length > 0.0).then_some(length)
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
        let Some(snapshot) = scene.read_component::<ComponentAnimator, _, _>(game_object, |c| {
            AnimatorSnapshot::from_component(c)
        }) else {
            return;
        };
        let skinned_meshes = scene
            .descendants_with::<ComponentSkinnedMesh>(game_object)
            .collect::<Vec<_>>();
        let Some(animation_graph) = snapshot.animation_graph.get_ref(assets) else {
            return;
        };
        let animation_graph = animation_graph.read();
        // Root-space transform of each posed bone by name, computed once per root bone and
        // shared by the skinned meshes under it.
        let mut skeletons: HashMap<GameObject, HashMap<String, Mat4>> = HashMap::new();
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
            let bones = skeletons.entry(root).or_insert_with(|| {
                let mut bones = HashMap::new();
                let transform = scene.transform(root);
                Self::traverse_bone_hierarchy_static(
                    &snapshot,
                    assets,
                    scene,
                    root,
                    &animation_graph,
                    &transform.inverse_matrix(),
                    Mat4::identity(),
                    &mut bones,
                );
                bones
            });
            ComponentSkinnedMesh::skinning_palette(
                &mesh_ref.read(),
                bones,
                &mut pose.bone_transforms,
            );
            scene.write_component::<ComponentSkinnedMesh, _>(skinned_mesh_go, |c_skinned_mesh| {
                c_skinned_mesh.bone_transforms.clear();
                c_skinned_mesh
                    .bone_transforms
                    .extend(pose.bone_transforms.drain(0..));
            });
        }
    }

    fn update_time(&mut self, time: &Time) {
        self.advance(time.delta_time());
    }

    fn advance(&mut self, delta_time: TimeType) {
        self.time += delta_time;
        self.state_time += delta_time;
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

    /// Poses the bones under `game_object`, writing their local transforms to the scene and
    /// their root-space transforms to `bones` by bone name.
    #[allow(clippy::too_many_arguments)]
    fn traverse_bone_hierarchy_static(
        snapshot: &AnimatorSnapshot,
        assets: &ReadOnlyRegistryContext,
        scene: &mut Scene,
        game_object: GameObject,
        animation_graph: &AnimationGraph,
        global_inverse_transform: &Mat4,
        mut parent_transform: Mat4,
        bones: &mut HashMap<String, Mat4>,
    ) {
        let mut local_transform = scene.transform(game_object).matrix();
        let mut captured_bind_pose = None;
        'calc_bone_transform: {
            let Some(entry) = scene.entry(game_object) else {
                break 'calc_bone_transform;
            };
            let Ok(c_bone) = entry.get_component::<ComponentBone>() else {
                break 'calc_bone_transform;
            };
            let bind_pose = c_bone
                .bind_pose
                .unwrap_or_else(|| scene.transform(game_object));
            if c_bone.bind_pose.is_none() {
                captured_bind_pose = Some(bind_pose);
            }
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
                snapshot.state_time,
                &bind_pose,
            )
            .matrix();
            bones.insert(
                c_bone.name.clone(),
                global_inverse_transform * parent_transform * local_transform,
            );
        }
        if let Some(bind_pose) = captured_bind_pose {
            scene.write_component::<ComponentBone, _>(game_object, |c_bone| {
                c_bone.bind_pose = Some(bind_pose);
            });
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
                bones,
            );
        }
    }

    /// The keys around `ticks`: the last key at or before it and the one after. Of keys sharing
    /// a time, the last is taken from that time on, so a pair of keys at one time switches value
    /// there, as an imported step does.
    fn find_keyframes<T, F: FnMut(&T) -> TimeType>(
        keyframes: &[T],
        ticks: TimeType,
        mut accessor: F,
    ) -> Option<(&T, &T)> {
        if keyframes.is_empty() {
            return None;
        }
        let index = keyframes.partition_point(|k| accessor(k) <= ticks) as isize;
        let max_index = (keyframes.len() - 1) as isize;
        let prev = (index - 1).clamp(0, max_index) as usize;
        let next = index.clamp(0, max_index) as usize;
        Some((&keyframes[prev], &keyframes[next]))
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
        bind_pose: &Transform,
    ) -> Transform {
        match motion {
            AnimationMotion::AnimationClip(clip) => {
                // A bone the clip does not animate keeps its bind pose.
                let Some(animation) = clip.animation.get_ref(assets) else {
                    return *bind_pose;
                };
                let animation = animation.read();
                let Some(keyframes) = animation.node_keyframes.get(bone_name) else {
                    return *bind_pose;
                };
                Self::animation_local_bone_transform(
                    keyframes,
                    (clip.speed * time * animation.ticks_per_second as f32)
                        % animation.duration as f32,
                    bind_pose,
                )
            }
            AnimationMotion::BlendTree1D(tree) => {
                if tree.motions.is_empty() {
                    crate::logging::warn_once(format!("empty-blend-tree:{tree:p}"), || {
                        "Blend tree has no motions; using bind pose".to_string()
                    });
                    return *bind_pose;
                }
                let neighbors = tree.nearest_neighbors(2, parameters);
                if neighbors.is_empty() {
                    return *bind_pose;
                }
                Transform::nlerp(neighbors.into_iter().map(|(weight, neighbor)| {
                    (
                        weight,
                        Self::motion_local_bone_transform_static(
                            parameters,
                            assets,
                            &neighbor.motion,
                            bone_name,
                            time,
                            bind_pose,
                        ),
                    )
                }))
            }
            AnimationMotion::BlendTree2D(tree) => {
                if tree.motions.is_empty() {
                    crate::logging::warn_once(format!("empty-blend-tree:{tree:p}"), || {
                        "Blend tree has no motions; using bind pose".to_string()
                    });
                    return *bind_pose;
                }
                let neighbors = tree.nearest_neighbors(4, parameters);
                if neighbors.is_empty() {
                    return *bind_pose;
                }
                Transform::nlerp(neighbors.into_iter().map(|(weight, neighbor)| {
                    (
                        weight,
                        Self::motion_local_bone_transform_static(
                            parameters,
                            assets,
                            &neighbor.motion,
                            bone_name,
                            time,
                            bind_pose,
                        ),
                    )
                }))
            }
        }
    }

    pub(crate) fn animation_local_bone_transform(
        keyframes: &AnimationKeyFrames,
        ticks: f32,
        bind_pose: &Transform,
    ) -> Transform {
        // Channels without keyframes keep the bind pose value.
        let position = Self::find_keyframes(&keyframes.positions, ticks, |k| k.time as f32)
            .map(|(prev, next)| Self::interpolate_vector(prev, next, ticks))
            .unwrap_or(bind_pose.position);
        let rotation = Self::find_keyframes(&keyframes.rotations, ticks, |k| k.time as f32)
            .map(|(prev, next)| Self::interpolate_quat(prev, next, ticks))
            .unwrap_or(bind_pose.rotation);
        let scale = Self::find_keyframes(&keyframes.scaling, ticks, |k| k.time as f32)
            .map(|(prev, next)| Self::interpolate_vector(prev, next, ticks))
            .unwrap_or(bind_pose.scale);

        Transform {
            position,
            rotation,
            scale,
        }
    }
}

#[cfg(test)]
mod tests;
