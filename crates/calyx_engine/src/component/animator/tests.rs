use super::*;
use crate::assets::animation::Animation;
use crate::assets::animation_graph::{AnimationClip, AnimationMotion, AnimationNode, BlendTree};
use crate::test_support::test_registries;

fn bind_pose() -> Transform {
    Transform::from_xyz(1.0, 2.0, 3.0)
}

#[test]
fn find_keyframes_on_empty_channel_returns_none() {
    let keyframes: Vec<VectorKeyFrame> = vec![];
    assert!(ComponentAnimator::find_keyframes(&keyframes, 0.0, |k| k.time as f32).is_none());
}

#[test]
fn keys_sharing_a_time_switch_value_at_that_time() {
    // A step from 1 to 2 at one tick, as the glTF importer writes it.
    let key = |time: f64, x: f32| VectorKeyFrame {
        value: Vec3::new(x, 0.0, 0.0),
        time,
    };
    let keyframes = AnimationKeyFrames {
        positions: vec![key(0.0, 1.0), key(1.0, 1.0), key(1.0, 2.0), key(2.0, 2.0)],
        rotations: vec![],
        scaling: vec![],
    };
    let x_at = |ticks: f32| {
        ComponentAnimator::animation_local_bone_transform(&keyframes, ticks, &bind_pose())
            .position
            .x
    };

    assert_eq!(x_at(0.5), 1.0);
    assert_eq!(x_at(0.999), 1.0);
    assert_eq!(x_at(1.0), 2.0);
    assert_eq!(x_at(1.5), 2.0);
    assert_eq!(x_at(2.0), 2.0);
}

#[test]
fn linear_keys_interpolate_and_hold_past_the_ends() {
    let key = |time: f64, x: f32| VectorKeyFrame {
        value: Vec3::new(x, 0.0, 0.0),
        time,
    };
    let keyframes = AnimationKeyFrames {
        positions: vec![key(1.0, 0.0), key(2.0, 4.0), key(3.0, 2.0)],
        rotations: vec![],
        scaling: vec![],
    };
    let x_at = |ticks: f32| {
        ComponentAnimator::animation_local_bone_transform(&keyframes, ticks, &bind_pose())
            .position
            .x
    };

    assert_eq!(x_at(0.0), 0.0);
    assert_eq!(x_at(1.5), 2.0);
    assert_eq!(x_at(2.0), 4.0);
    assert_eq!(x_at(2.5), 3.0);
    assert_eq!(x_at(4.0), 2.0);
}

#[test]
fn empty_channels_keep_bind_pose() {
    let keyframes = AnimationKeyFrames {
        positions: vec![],
        rotations: vec![],
        scaling: vec![],
    };
    let result = ComponentAnimator::animation_local_bone_transform(&keyframes, 0.5, &bind_pose());
    assert_eq!(result.position, bind_pose().position);
    assert_eq!(result.rotation, bind_pose().rotation);
    assert_eq!(result.scale, bind_pose().scale);
}

#[test]
fn bone_without_a_channel_keeps_bind_pose() {
    let assets = test_registries();
    let other_bone = AnimationKeyFrames {
        positions: vec![VectorKeyFrame {
            value: Vec3::new(9.0, 9.0, 9.0),
            time: 0.0,
        }],
        rotations: vec![],
        scaling: vec![],
    };
    let animation = assets
        .assets
        .read()
        .create(
            "partial_clip".to_string(),
            Animation {
                node_keyframes: HashMap::from([("other".to_string(), other_bone)]),
                duration: 1.0,
                ticks_per_second: 1.0,
            },
        )
        .unwrap();
    let motion = AnimationMotion::AnimationClip(AnimationClip {
        speed: 1.0,
        animation: AssetRef::from_id(animation.id()),
    });

    let result = ComponentAnimator::motion_local_bone_transform_static(
        &HashMap::new(),
        &assets,
        &motion,
        "bone",
        0.5,
        &bind_pose(),
    );

    assert_eq!(result.position, bind_pose().position);
    assert_eq!(result.rotation, bind_pose().rotation);
    assert_eq!(result.scale, bind_pose().scale);
}

/// The `TwoSkins` model: two meshes skinned by one joint tree, the body's skin listing
/// [Hip, Spine] and the arm's [Arm, Spine, Hip], instantiated with an animator playing its
/// one-second bend clip. Returns the assets, which own the registries, and the scene with
/// its root.
fn two_skins_scene() -> (
    crate::test_support::TestAssets,
    ReadOnlyRegistryContext,
    Scene,
    GameObject,
) {
    let test_assets = crate::test_support::TestAssets::models();
    let assets = test_assets.registries();
    let path = test_assets.fixtures_root().join("gltf/TwoSkins.gltf");
    let registry = assets.assets.read();
    let prefab = registry
        .load_by_path::<crate::scene::Prefab>(&path)
        .unwrap();
    let animation = registry
        .asset_meta_from_path(&path)
        .unwrap()
        .children
        .into_iter()
        .find(|id| registry.loaded_by_id::<Animation>(*id).is_some())
        .unwrap();
    let mut graph = AnimationGraph::default();
    let start = Uuid::new_v4();
    graph.add_node(AnimationNode {
        id: start,
        name: "Bend".to_string(),
        motion: AnimationMotion::AnimationClip(AnimationClip {
            speed: 1.0,
            animation: AssetRef::from_id(animation),
        }),
        position: Default::default(),
    });
    graph.start_node = Some(start);
    let graph = registry
        .create("two_skins_graph".to_string(), graph)
        .unwrap();
    drop(registry);

    let mut scene = assets.scene();
    let root = scene.instantiate_prefab(&prefab.read(), None).unwrap();
    scene.add_component(
        root,
        ComponentAnimator {
            animation_graph: AssetRef::from_id(graph.id()),
            ..Default::default()
        },
    );
    (test_assets, assets, scene, root)
}

/// The animator's time and each skinned mesh's palette, as bytes so poses compare.
fn pose_of(scene: &Scene, root: GameObject) -> (TimeType, Vec<Vec<u8>>) {
    let time = scene
        .read_component::<ComponentAnimator, _, _>(root, |c| c.time)
        .unwrap();
    let poses = scene
        .descendants_with::<ComponentSkinnedMesh>(root)
        .map(|mesh| {
            scene
                .read_component::<ComponentSkinnedMesh, _, _>(mesh, |c| {
                    bytemuck::cast_slice::<BoneTransform, u8>(&c.bone_transforms).to_vec()
                })
                .unwrap()
        })
        .collect();
    (time, poses)
}

#[test]
fn a_disabled_animator_neither_advances_nor_poses_until_re_enabled() {
    let (_test_assets, _assets, mut scene, root) = two_skins_scene();
    assert!(scene.set_component_enabled::<ComponentAnimator>(root, false));
    let mut runner = crate::test_support::HeadlessSceneRunner::from_scene(scene);
    runner.step_many(2);
    let (time, poses) = pose_of(runner.scene(), root);
    assert_eq!(time, 0.0);
    assert!(
        poses.iter().all(Vec::is_empty),
        "a mesh never posed keeps drawing its rest pose"
    );

    assert!(runner
        .scene_mut()
        .set_component_enabled::<ComponentAnimator>(root, true));
    runner.step();
    let (posed_time, posed) = pose_of(runner.scene(), root);
    assert!(posed_time > 0.0);
    assert!(posed.iter().all(|pose| !pose.is_empty()));

    assert!(runner
        .scene_mut()
        .set_component_enabled::<ComponentAnimator>(root, false));
    runner.step_many(3);
    assert_eq!(
        pose_of(runner.scene(), root),
        (posed_time, posed.clone()),
        "a disabled animator keeps its meshes in the last pose it wrote"
    );

    assert!(runner
        .scene_mut()
        .set_component_enabled::<ComponentAnimator>(root, true));
    runner.step();
    let (time, poses) = pose_of(runner.scene(), root);
    assert!(
        time > posed_time,
        "enabled again, it advances from where it stopped"
    );
    assert_ne!(poses, posed);
}

#[test]
fn skinned_meshes_ordering_bones_differently_each_follow_their_own_bones() {
    // Halfway through the clip the hip has moved 1 along z and the spine has turned 45
    // degrees about z; the arm on the spine has no channel and turns with it.
    let (_test_assets, assets, mut scene, root) = two_skins_scene();
    scene.write_component::<ComponentAnimator, _>(root, |c| {
        c.init(&assets);
        c.advance(0.5);
    });
    ComponentAnimator::compute_animation_pose(
        &assets,
        &mut scene,
        root,
        &mut AnimatorPose::default(),
    );

    let hip = nalgebra_glm::translation(&Vec3::new(0.0, 0.0, 1.0));
    let spine = nalgebra_glm::translation(&Vec3::new(0.0, 1.0, 1.0))
        * nalgebra_glm::rotation(std::f32::consts::FRAC_PI_4, &Vec3::z())
        * nalgebra_glm::translation(&Vec3::new(0.0, -1.0, 0.0));
    let expected = HashMap::from([("Hip", hip), ("Spine", spine), ("Arm", spine)]);
    let skinned_meshes = scene
        .descendants_with::<ComponentSkinnedMesh>(root)
        .collect::<Vec<_>>();
    assert_eq!(skinned_meshes.len(), 2);
    for skinned_mesh in skinned_meshes {
        let (mesh, bone_transforms) = scene
            .read_component::<ComponentSkinnedMesh, _, _>(skinned_mesh, |c| {
                (c.mesh.get_ref(&assets).unwrap(), c.bone_transforms.clone())
            })
            .unwrap();
        let mesh = mesh.read();
        assert_eq!(bone_transforms.len(), mesh.bones.len());
        for (vertex, indices) in mesh.vertices.iter().zip(&mesh.bone_indices) {
            // Every vertex follows a single bone with full weight.
            let index = indices.x as usize;
            let name = mesh
                .bones
                .iter()
                .find_map(|(name, bone)| (bone.index == index).then_some(name.as_str()))
                .unwrap();
            let point = vertex.push(1.0);
            let skinned = Mat4::from(bone_transforms[index].transform) * point;
            let want = expected[name] * point;
            assert!(
                (skinned - want).norm() < 1e-5,
                "vertex {vertex:?} of a mesh following {name}: got {skinned:?}, want {want:?}"
            );
        }
    }
}

/// A hand-built state machine: states named by [`ExitTimeGraph::state`], transitions
/// between them, and one bool parameter `go` that every conditioned transition checks.
struct ExitTimeGraph {
    assets: ReadOnlyRegistryContext,
    graph: AnimationGraph,
    go: Uuid,
}

impl ExitTimeGraph {
    fn new() -> Self {
        let go = Uuid::new_v4();
        let mut graph = AnimationGraph::default();
        graph
            .parameters
            .push(crate::assets::animation_graph::AnimationParameter {
                id: go,
                name: "go".to_string(),
                value: AnimationParameterValue::Bool(false),
            });
        Self {
            assets: test_registries(),
            graph,
            go,
        }
    }

    /// A clip `seconds` long at one tick per second, played at `speed`.
    fn clip(&self, seconds: f64, speed: f32) -> AnimationMotion {
        let animation = self
            .assets
            .assets
            .read()
            .create(
                format!("clip_{}", Uuid::new_v4()),
                Animation {
                    node_keyframes: HashMap::new(),
                    duration: seconds,
                    ticks_per_second: 1.0,
                },
            )
            .unwrap();
        AnimationMotion::AnimationClip(AnimationClip {
            speed,
            animation: AssetRef::from_id(animation.id()),
        })
    }

    /// Adds a state; the first one added is the start state.
    fn state(&mut self, name: &str, motion: AnimationMotion) -> NodeIndex {
        let id = Uuid::new_v4();
        if self.graph.start_node.is_none() {
            self.graph.start_node = Some(id);
        }
        self.graph.add_node(AnimationNode {
            id,
            name: name.to_string(),
            motion,
            position: Default::default(),
        })
    }

    /// Adds an instant transition. `exit_time` sets `has_exit_time`; `conditioned` makes it
    /// wait for `go` to be true.
    fn transition(
        &mut self,
        from: NodeIndex,
        to: NodeIndex,
        exit_time: Option<f32>,
        conditioned: bool,
    ) {
        let conditions = if conditioned {
            vec![AnimationParameterCondition {
                parameter: self.go,
                condition: AnimationCondition::Bool(BoolCondition::True),
            }]
        } else {
            vec![]
        };
        self.graph.add_edge(
            from,
            to,
            AnimationTransition {
                id: Uuid::new_v4(),
                name: String::new(),
                has_exit_time: exit_time.is_some(),
                exit_time: exit_time.unwrap_or(0.0),
                duration: 0.0,
                conditions,
            },
        );
    }

    fn animator(self) -> AnimatorUnderTest {
        let graph = self
            .assets
            .assets
            .read()
            .create("exit_time_graph".to_string(), self.graph)
            .unwrap();
        let mut animator = ComponentAnimator {
            animation_graph: AssetRef::from_id(graph.id()),
            ..Default::default()
        };
        animator.init(&self.assets);
        AnimatorUnderTest {
            assets: self.assets,
            animator,
            go: self.go,
        }
    }
}

/// Steps an animator the way [`ComponentAnimator::update`] does, in frames of
/// [`AnimatorUnderTest::FRAME`] seconds, which binary floats hold exactly.
struct AnimatorUnderTest {
    assets: ReadOnlyRegistryContext,
    animator: ComponentAnimator,
    go: Uuid,
}

impl AnimatorUnderTest {
    const FRAME: f32 = 0.125;

    fn set_go(&mut self, value: bool) {
        self.animator
            .parameters
            .insert(self.go, AnimationParameterValue::Bool(value));
    }

    /// Runs one frame: the state machine sees the time reached so far, then time advances.
    fn frame(&mut self) {
        self.animator.init(&self.assets);
        self.animator.step_fsm(&self.assets);
        self.animator.advance(Self::FRAME);
    }

    /// Runs frames until the state machine has seen `seconds` of time in the current state.
    fn run_to(&mut self, seconds: f32) {
        while self.animator.state_time <= seconds {
            self.frame();
        }
    }

    fn state(&self) -> String {
        let graph = self.animator.animation_graph.get_ref(&self.assets).unwrap();
        let graph = graph.read();
        graph[self.animator.current_state.unwrap()].name.clone()
    }

    fn transitioning(&self) -> bool {
        self.animator.current_transition.is_some()
    }
}

#[test]
fn exit_time_transition_waits_for_the_exit_time() {
    let mut graph = ExitTimeGraph::new();
    let a = graph.state("A", graph.clip(1.0, 1.0));
    let b = graph.state("B", graph.clip(1.0, 1.0));
    graph.transition(a, b, Some(0.75), true);
    let mut animator = graph.animator();
    animator.set_go(true);

    animator.run_to(0.625);
    assert!(!animator.transitioning(), "started before the exit time");

    animator.run_to(0.75);
    assert!(animator.transitioning(), "did not start at the exit time");
    animator.frame();
    assert_eq!(animator.state(), "B");
}

#[test]
fn exit_time_transition_still_needs_its_conditions() {
    let mut graph = ExitTimeGraph::new();
    let a = graph.state("A", graph.clip(1.0, 1.0));
    let b = graph.state("B", graph.clip(1.0, 1.0));
    graph.transition(a, b, Some(0.75), true);
    let mut animator = graph.animator();

    animator.run_to(0.875);
    assert!(!animator.transitioning());
    assert_eq!(animator.state(), "A");
}

#[test]
fn exit_time_transition_without_conditions_flows_on_when_the_clip_ends() {
    // A clip of 2 ticks at speed 2 lasts one second.
    let mut graph = ExitTimeGraph::new();
    let a = graph.state("A", graph.clip(2.0, 2.0));
    let b = graph.state("B", graph.clip(1.0, 1.0));
    graph.transition(a, b, Some(1.0), false);
    let mut animator = graph.animator();

    animator.run_to(0.875);
    assert!(!animator.transitioning());

    animator.run_to(1.0);
    assert!(animator.transitioning());
    animator.frame();
    assert_eq!(animator.state(), "B");
}

#[test]
fn graph_without_parameters_flows_on_at_the_exit_time() {
    let mut graph = ExitTimeGraph::new();
    graph.graph.parameters.clear();
    let a = graph.state("A", graph.clip(1.0, 1.0));
    let b = graph.state("B", graph.clip(1.0, 1.0));
    graph.transition(a, b, Some(1.0), false);
    let mut animator = graph.animator();

    animator.run_to(1.0);
    animator.frame();
    assert_eq!(animator.state(), "B");
    animator.frame();
    assert_eq!(animator.state(), "B", "returned to the start state");
}

#[test]
fn exit_time_below_one_fires_at_that_point_of_the_loop_the_conditions_pass_in() {
    let mut graph = ExitTimeGraph::new();
    let a = graph.state("A", graph.clip(1.0, 1.0));
    let b = graph.state("B", graph.clip(1.0, 1.0));
    graph.transition(a, b, Some(0.75), true);
    let mut animator = graph.animator();

    // The conditions pass a quarter into the second loop: wait for 75% of that loop.
    animator.run_to(1.125);
    animator.set_go(true);
    animator.run_to(1.625);
    assert!(!animator.transitioning(), "started before 75% of the loop");
    animator.run_to(1.75);
    assert!(animator.transitioning(), "did not start at 75% of the loop");
}

#[test]
fn exit_time_below_one_fires_at_once_when_the_conditions_pass_past_it() {
    let mut graph = ExitTimeGraph::new();
    let a = graph.state("A", graph.clip(1.0, 1.0));
    let b = graph.state("B", graph.clip(1.0, 1.0));
    graph.transition(a, b, Some(0.75), true);
    let mut animator = graph.animator();

    animator.run_to(1.75);
    animator.set_go(true);
    animator.frame();
    assert!(animator.transitioning());
}

#[test]
fn exit_time_above_one_counts_loops() {
    let mut graph = ExitTimeGraph::new();
    let a = graph.state("A", graph.clip(1.0, 1.0));
    let b = graph.state("B", graph.clip(1.0, 1.0));
    graph.transition(a, b, Some(1.5), true);
    let mut animator = graph.animator();
    animator.set_go(true);

    animator.run_to(1.375);
    assert!(!animator.transitioning(), "started before 1.5 loops");
    animator.run_to(1.5);
    assert!(animator.transitioning(), "did not start at 1.5 loops");
}

#[test]
fn exit_time_above_one_fires_at_its_fraction_of_each_later_loop() {
    let mut graph = ExitTimeGraph::new();
    let a = graph.state("A", graph.clip(1.0, 1.0));
    let b = graph.state("B", graph.clip(1.0, 1.0));
    graph.transition(a, b, Some(1.5), true);
    let mut animator = graph.animator();

    // The conditions pass a quarter into the third loop: wait for half of that loop.
    animator.run_to(2.125);
    animator.set_go(true);
    animator.run_to(2.375);
    assert!(!animator.transitioning(), "started before half of the loop");
    animator.run_to(2.5);
    assert!(
        animator.transitioning(),
        "did not start at half of the loop"
    );
}

#[test]
fn exit_time_stepped_over_at_a_loop_boundary_still_fires() {
    // Frames see 0.875 and then 1.0 (fraction 0): no frame lands past 0.95 in a loop.
    let mut graph = ExitTimeGraph::new();
    let a = graph.state("A", graph.clip(1.0, 1.0));
    let b = graph.state("B", graph.clip(1.0, 1.0));
    graph.transition(a, b, Some(0.95), false);
    let mut animator = graph.animator();

    animator.run_to(0.875);
    assert!(!animator.transitioning(), "started before the exit time");
    animator.run_to(1.0);
    assert!(
        animator.transitioning(),
        "missed the exit time it stepped over"
    );
}

#[test]
fn exit_time_above_one_stepped_over_at_a_loop_boundary_still_fires() {
    // A clip of 1 tick at speed 4 lasts a quarter second, so each frame is half a loop and
    // frames only ever land on fractions 0 and 0.5.
    let mut graph = ExitTimeGraph::new();
    let a = graph.state("A", graph.clip(1.0, 4.0));
    let b = graph.state("B", graph.clip(1.0, 1.0));
    graph.transition(a, b, Some(1.95), true);
    let mut animator = graph.animator();
    animator.set_go(true);

    animator.run_to(0.375);
    assert!(!animator.transitioning(), "started before 1.95 loops");
    animator.run_to(0.5);
    assert!(
        animator.transitioning(),
        "missed the exit time it stepped over"
    );
}

#[test]
fn transition_without_exit_time_fires_when_its_conditions_pass() {
    let mut graph = ExitTimeGraph::new();
    let a = graph.state("A", graph.clip(1.0, 1.0));
    let b = graph.state("B", graph.clip(1.0, 1.0));
    graph.transition(a, b, None, true);
    let mut animator = graph.animator();

    animator.run_to(0.25);
    assert!(!animator.transitioning());
    animator.set_go(true);
    animator.frame();
    assert!(animator.transitioning());
}

#[test]
fn exit_time_counts_from_when_the_state_was_entered() {
    let mut graph = ExitTimeGraph::new();
    let a = graph.state("A", graph.clip(1.0, 1.0));
    let b = graph.state("B", graph.clip(1.0, 1.0));
    let c = graph.state("C", graph.clip(1.0, 1.0));
    graph.transition(a, b, None, true);
    graph.transition(b, c, Some(0.5), false);
    let mut animator = graph.animator();

    animator.run_to(0.625);
    animator.set_go(true);
    animator.frame();
    animator.frame();
    assert_eq!(animator.state(), "B");

    animator.run_to(0.375);
    assert!(!animator.transitioning(), "B's exit time counted A's time");
    animator.run_to(0.5);
    assert!(animator.transitioning());
}

#[test]
fn blend_tree_state_length_is_the_blend_weighted_clip_length() {
    // Halfway between a 1 s clip and a 3 s clip, both weigh half: the state lasts 2 s.
    let mut graph = ExitTimeGraph::new();
    let blend = Uuid::new_v4();
    graph
        .graph
        .parameters
        .push(crate::assets::animation_graph::AnimationParameter {
            id: blend,
            name: "blend".to_string(),
            value: AnimationParameterValue::Float(0.5),
        });
    let tree = BlendTree::<1> {
        parameters: vec![blend],
        motions: vec![
            crate::assets::animation_graph::BlendTreeMotion {
                threshold: [0.0],
                motion: graph.clip(1.0, 1.0),
            },
            crate::assets::animation_graph::BlendTreeMotion {
                threshold: [1.0],
                motion: graph.clip(3.0, 1.0),
            },
        ],
    };
    let a = graph.state("A", AnimationMotion::BlendTree1D(tree));
    let b = graph.state("B", graph.clip(1.0, 1.0));
    graph.transition(a, b, Some(0.5), false);
    let mut animator = graph.animator();

    animator.run_to(0.875);
    assert!(!animator.transitioning(), "used a length under 2 s");
    animator.run_to(1.0);
    assert!(animator.transitioning(), "used a length over 2 s");
}

#[test]
fn empty_blend_tree_evaluates_to_bind_pose() {
    let assets = test_registries();
    let motion = AnimationMotion::BlendTree1D(BlendTree::<1>::default());
    let result = ComponentAnimator::motion_local_bone_transform_static(
        &HashMap::new(),
        &assets,
        &motion,
        "bone",
        0.0,
        &bind_pose(),
    );
    assert_eq!(result.position, bind_pose().position);
    assert!(!result.rotation.coords.iter().any(|c| c.is_nan()));
}
