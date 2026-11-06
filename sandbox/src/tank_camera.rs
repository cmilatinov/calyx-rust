//! The gameplay camera rig: the tank's camera framing, aim-ahead and zoom.

use calyx_engine::component::{Component, ComponentCamera, ReflectComponent};
use calyx_engine::input::Input;
use calyx_engine::math::Transform;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::render::Ray;
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec2, vec3, Vec3};
use serde::{Deserialize, Serialize};

/// Frames the player's tank from above: the camera sits `height` up and
/// `back` behind its focus, looking down at it, leads towards the point the
/// player aims at, and zooms between a near and a far framing with the
/// scroll wheel. The tank controller whose `camera` names this object moves
/// it every frame, before it aims through it.
#[derive(Clone, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "7463cb88-5789-490e-a46d-50e30a6d61d9"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Tank Camera")]
#[serde(default)]
pub struct ComponentTankCamera {
    /// Height above the focus at a zoom of 1, in metres.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub height: f32,
    /// Distance behind the focus at a zoom of 1, in metres. The camera
    /// looks down at the focus, so its pitch is `atan(height / back)`.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub back: f32,
    /// Zoom the camera starts at: a scale on `height` and `back`.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub zoom: f32,
    /// Nearest zoom the scroll wheel reaches.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub min_zoom: f32,
    /// Farthest zoom the scroll wheel reaches.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub max_zoom: f32,
    /// Zoom change per point of scroll: scrolling up zooms in.
    #[reflect_attr(min = 0.0, speed = 0.0001)]
    pub zoom_sensitivity: f32,
    /// Time constant the zoom eases towards its target with, in seconds;
    /// 0 snaps.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub zoom_smoothing: f32,
    /// Share of the distance from the tank to the aim point, beyond
    /// `aim_ahead_dead_zone`, that the focus leads by.
    #[reflect_attr(min = 0.0, max = 0.9, speed = 0.01)]
    pub aim_ahead: f32,
    /// Farthest the focus leads from the tank, in metres.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub aim_ahead_max: f32,
    /// Distance from the tank within which aiming does not move the focus,
    /// in metres: aiming near the tank recentres it.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub aim_ahead_dead_zone: f32,
    /// Time constant the lead eases towards its target with, in seconds;
    /// 0 snaps.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub aim_ahead_smoothing: f32,
    /// Where the rig is this frame; set on its first update.
    #[serde(skip)]
    #[reflect_skip]
    state: Option<RigState>,
}

/// The rig's eased values.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RigState {
    /// Ground offset of the focus from the tank.
    lead: Vec3,
    /// Zoom the scroll wheel asks for.
    target_zoom: f32,
    /// Zoom the camera shows.
    zoom: f32,
}

impl Default for ComponentTankCamera {
    fn default() -> Self {
        Self {
            height: 34.0,
            back: 13.7,
            zoom: 1.0,
            min_zoom: 0.6,
            max_zoom: 1.0,
            zoom_sensitivity: 0.002,
            zoom_smoothing: 0.12,
            aim_ahead: 0.25,
            aim_ahead_max: 6.0,
            aim_ahead_dead_zone: 4.0,
            aim_ahead_smoothing: 0.35,
            state: None,
        }
    }
}

impl Component for ComponentTankCamera {}

impl ComponentTankCamera {
    /// The ground offset the focus leads the tank at `tank` by, while the
    /// player aims at `aim`.
    pub fn aim_ahead_target(&self, tank: Vec3, aim: Vec3) -> Vec3 {
        let ground = vec3(aim.x - tank.x, 0.0, aim.z - tank.z);
        let distance = ground.magnitude();
        let past_dead_zone = distance - self.aim_ahead_dead_zone.max(0.0);
        if past_dead_zone <= 0.0 || distance <= f32::EPSILON {
            return Vec3::zeros();
        }
        let lead =
            (past_dead_zone * self.aim_ahead.clamp(0.0, 0.9)).min(self.aim_ahead_max.max(0.0));
        ground * (lead / distance)
    }

    /// The zoom the scroll wheel asks for after `scroll` points of scroll
    /// from `zoom`, within the zoom limits.
    pub fn zoom_after_scroll(&self, zoom: f32, scroll: f32) -> f32 {
        self.clamp_zoom(zoom - scroll * self.zoom_sensitivity)
    }

    /// `zoom` within the zoom limits.
    fn clamp_zoom(&self, zoom: f32) -> f32 {
        let near = self.min_zoom.max(0.0);
        zoom.clamp(near, self.max_zoom.max(near))
    }

    /// The camera's transform when it frames `focus` at `zoom`, turned
    /// `yaw` radians about the vertical axis.
    pub fn camera_transform(&self, focus: Vec3, yaw: f32, zoom: f32) -> Transform {
        let turn = UnitQuaternion::from_euler_angles(0.0, yaw, 0.0);
        let offset = turn * vec3(0.0, self.height, -self.back) * zoom;
        let pitch = self.height.atan2(self.back);
        Transform::from_components(
            focus + offset,
            UnitQuaternion::from_euler_angles(pitch, yaw, 0.0),
            vec3(1.0, 1.0, 1.0),
        )
    }
}

/// Eases `current` towards `target` over `dt` seconds with the time
/// constant `smoothing`: the same total for any split of the time into
/// frames.
pub fn ease<T>(current: T, target: T, smoothing: f32, dt: f32) -> T
where
    T: std::ops::Sub<Output = T>
        + std::ops::Add<Output = T>
        + std::ops::Mul<f32, Output = T>
        + Copy,
{
    if smoothing <= 0.0 {
        return target;
    }
    let share = 1.0 - (-dt.max(0.0) / smoothing).exp();
    current + (target - current) * share
}

/// Moves `camera_object`'s rig to frame the tank at `tank`, easing its lead
/// and zoom over `dt`. Returns false, changing nothing, when the object has
/// no [`ComponentTankCamera`].
pub(crate) fn update_tank_camera(
    scene: &mut Scene,
    camera_object: GameObject,
    input: &Input,
    dt: f32,
    tank: Vec3,
) -> bool {
    let Some(rig) = scene.read_component::<ComponentTankCamera, _, _>(camera_object, Clone::clone)
    else {
        return false;
    };
    let mut state = rig.state.unwrap_or_else(|| {
        let zoom = rig.clamp_zoom(rig.zoom);
        RigState {
            lead: Vec3::zeros(),
            target_zoom: zoom,
            zoom,
        }
    });

    // Keep the yaw the camera was placed with.
    let forward = scene.world_transform(camera_object).forward();
    let yaw = if forward.x.abs() + forward.z.abs() > f32::EPSILON {
        forward.x.atan2(forward.z)
    } else {
        0.0
    };

    let scroll = if input.action("scoreboard").pressed() {
        0.0
    } else {
        input.scroll_delta().y
    };
    state.target_zoom = rig.zoom_after_scroll(state.target_zoom, scroll);
    state.zoom = rig.clamp_zoom(ease(state.zoom, state.target_zoom, rig.zoom_smoothing, dt));

    // Aim through the camera as the lead left it, then lead from there: the
    // lead settles where it is `aim_ahead` of the way to the aim point.
    let placed = rig.camera_transform(tank + state.lead, yaw, state.zoom);
    let target_lead = cursor_ground_point(scene, input, camera_object, &placed, tank.y)
        .map(|aim| rig.aim_ahead_target(tank, aim))
        .unwrap_or_else(Vec3::zeros);
    state.lead = ease(state.lead, target_lead, rig.aim_ahead_smoothing, dt);

    let transform = rig.camera_transform(tank + state.lead, yaw, state.zoom);
    scene.set_world_transform(camera_object, transform.matrix());
    let _ = scene.write_component::<ComponentTankCamera, _>(camera_object, |rig| {
        rig.state = Some(state);
    });
    true
}

/// Where the cursor's ray through `camera_object`, placed at
/// `camera_transform`, meets the horizontal plane at `ground_y`.
pub(crate) fn cursor_ground_point(
    scene: &Scene,
    input: &Input,
    camera_object: GameObject,
    camera_transform: &Transform,
    ground_y: f32,
) -> Option<Vec3> {
    let rect = input.surface_rect()?;
    let cursor = input.pointer_pos()?;
    let aspect = rect.width() / rect.height().max(f32::EPSILON);
    let camera = scene
        .read_component::<ComponentCamera, _, _>(camera_object, |component| {
            component.camera(aspect)
        })
        .unwrap_or_else(|| ComponentCamera::default().camera(aspect));
    let ray = camera.screen_ray(
        camera_transform,
        vec2(cursor.x - rect.left(), cursor.y - rect.top()),
        vec2(rect.width(), rect.height()),
    )?;
    ground_intersection(&ray, ground_y)
}

/// Where `ray` meets the horizontal plane at `ground_y`, ahead of its
/// origin.
pub(crate) fn ground_intersection(ray: &Ray, ground_y: f32) -> Option<Vec3> {
    if ray.direction.y.abs() <= f32::EPSILON {
        return None;
    }

    let t = (ground_y - ray.origin.y) / ray.direction.y;
    (t >= 0.0).then_some(ray.origin + ray.direction * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use calyx_engine::render::Camera;
    use nalgebra_glm::{vec2, vec4, Vec2};

    fn far_camera(aspect: f32) -> Camera {
        Camera::new(aspect, 64.0f32.to_radians(), 0.1, 100.0)
    }

    #[test]
    fn aim_ahead_leads_towards_the_aim_point_on_each_side() {
        let rig = ComponentTankCamera::default();
        let tank = vec3(2.0, 0.0, 3.0);

        // 10 m away, 6 m past the 4 m dead zone, a quarter of that is 1.5 m.
        let right = rig.aim_ahead_target(tank, tank + vec3(10.0, 0.0, 0.0));
        let left = rig.aim_ahead_target(tank, tank + vec3(-10.0, 0.0, 0.0));
        let ahead = rig.aim_ahead_target(tank, tank + vec3(0.0, 0.0, 10.0));

        assert!(
            (right - vec3(1.5, 0.0, 0.0)).magnitude() < 1e-5,
            "{right:?}"
        );
        assert!((left - vec3(-1.5, 0.0, 0.0)).magnitude() < 1e-5, "{left:?}");
        assert!(
            (ahead - vec3(0.0, 0.0, 1.5)).magnitude() < 1e-5,
            "{ahead:?}"
        );
    }

    #[test]
    fn aim_ahead_ignores_the_aim_point_height() {
        let rig = ComponentTankCamera::default();
        let lead = rig.aim_ahead_target(Vec3::zeros(), vec3(10.0, 5.0, 0.0));

        assert!((lead - vec3(1.5, 0.0, 0.0)).magnitude() < 1e-5, "{lead:?}");
    }

    #[test]
    fn aim_ahead_is_capped_at_its_maximum_distance() {
        let rig = ComponentTankCamera::default();
        let lead = rig.aim_ahead_target(Vec3::zeros(), vec3(-60.0, 0.0, 80.0));

        assert!((lead - vec3(-3.6, 0.0, 4.8)).magnitude() < 1e-4, "{lead:?}");
    }

    #[test]
    fn aiming_near_the_tank_recentres_the_camera() {
        let rig = ComponentTankCamera::default();

        for aim in [Vec3::zeros(), vec3(3.0, 0.0, 0.0), vec3(0.0, 0.0, -4.0)] {
            assert_eq!(rig.aim_ahead_target(Vec3::zeros(), aim), Vec3::zeros());
        }
    }

    #[test]
    fn easing_moves_part_way_and_does_not_depend_on_the_frame_rate() {
        let target = vec3(4.0, 0.0, -2.0);

        let one_frame = ease(Vec3::zeros(), target, 0.35, 0.2);
        let two_frames = ease(ease(Vec3::zeros(), target, 0.35, 0.1), target, 0.35, 0.1);
        let settled = (0..600).fold(Vec3::zeros(), |lead, _| {
            ease(lead, target, 0.35, 1.0 / 60.0)
        });

        // 1 - e^(-0.2 / 0.35) of the way.
        assert!(
            (one_frame - target * 0.435_282).magnitude() < 1e-4,
            "{one_frame:?}"
        );
        assert!((one_frame - two_frames).magnitude() < 1e-5);
        assert!((settled - target).magnitude() < 1e-4);
    }

    #[test]
    fn easing_with_no_smoothing_snaps() {
        assert_eq!(ease(1.0, 3.0, 0.0, 0.016), 3.0);
    }

    #[test]
    fn scrolling_zooms_within_the_limits() {
        let rig = ComponentTankCamera::default();

        // Scrolling up zooms in: 100 points at 0.002 per point.
        assert!((rig.zoom_after_scroll(1.0, 100.0) - 0.8).abs() < 1e-5);
        assert!((rig.zoom_after_scroll(0.8, -50.0) - 0.9).abs() < 1e-5);
        assert_eq!(rig.zoom_after_scroll(0.8, 10_000.0), 0.6);
        assert_eq!(rig.zoom_after_scroll(0.8, -10_000.0), 1.0);
    }

    #[test]
    fn far_framing_sits_34_m_up_and_13_7_m_back_pitched_68_degrees() {
        let rig = ComponentTankCamera::default();
        let transform = rig.camera_transform(vec3(5.0, 0.0, -2.0), 0.0, 1.0);
        let forward = transform.forward();
        let pitch = (-forward.y).atan2(forward.z).to_degrees();

        assert!((transform.position - vec3(5.0, 34.0, -15.7)).magnitude() < 1e-4);
        assert!((pitch - 68.0).abs() < 0.1, "pitch {pitch}");
        assert!(forward.x.abs() < 1e-5);
    }

    #[test]
    fn zoom_scales_the_framing_and_keeps_the_pitch() {
        let rig = ComponentTankCamera::default();
        let near = rig.camera_transform(Vec3::zeros(), 0.0, 0.6);

        assert!((near.position - vec3(0.0, 20.4, -8.22)).magnitude() < 1e-4);
        assert!(
            near.rotation
                .angle_to(&rig.camera_transform(Vec3::zeros(), 0.0, 1.0).rotation)
                < 1e-5
        );
    }

    #[test]
    fn yaw_turns_the_framing_about_the_focus() {
        let rig = ComponentTankCamera::default();
        let turned = rig.camera_transform(Vec3::zeros(), std::f32::consts::FRAC_PI_2, 1.0);

        // Turned to look down +X, the camera sits back along -X.
        assert!((turned.position - vec3(-13.7, 34.0, 0.0)).magnitude() < 1e-4);
        assert!(turned.forward().x > 0.0);
    }

    #[test]
    fn far_framing_shows_about_46_m_of_ground_across_the_tank() {
        let rig = ComponentTankCamera::default();
        let transform = rig.camera_transform(Vec3::zeros(), 0.0, 1.0);
        let viewport = vec2(1920.0, 1080.0);
        let camera = far_camera(viewport.x / viewport.y);
        let edge = |x: f32| {
            let ray = camera
                .screen_ray(&transform, vec2(x, viewport.y * 0.5), viewport)
                .expect("edge ray");
            ground_intersection(&ray, 0.0).expect("edge ray meets the ground")
        };

        let width = (edge(viewport.x) - edge(0.0)).magnitude();

        assert!((width - 45.8).abs() < 0.1, "width {width}");
    }

    /// The components of the object named "Camera" in a saved scene or
    /// prefab.
    fn saved_camera(source: &str) -> serde_json::Value {
        let file: serde_json::Value = serde_json::from_str(source).expect("valid JSON");
        let components = file
            .get("data")
            .unwrap_or(&file)
            .get("components")
            .and_then(|components| components.as_object())
            .expect("component data");
        components
            .values()
            .find(|components| {
                components[calyx_engine::component::ComponentID::type_uuid().to_string()]["name"]
                    == "Camera"
            })
            .expect("a Camera object")
            .clone()
    }

    #[test]
    fn the_tank_prefab_and_the_arena_frame_the_tank_from_far() {
        for source in [
            include_str!("../assets/game/prefabs/tank.cxprefab"),
            include_str!("../assets/game/scenes/arena.cxscene"),
        ] {
            let camera = saved_camera(source);
            let rig: ComponentTankCamera = serde_json::from_value(
                camera[ComponentTankCamera::type_uuid().to_string()].clone(),
            )
            .expect("the camera has a tank camera rig");
            let lens: ComponentCamera =
                serde_json::from_value(camera[ComponentCamera::type_uuid().to_string()].clone())
                    .expect("the camera has a camera");
            let transform: calyx_engine::component::ComponentTransform = serde_json::from_value(
                camera[calyx_engine::component::ComponentTransform::type_uuid().to_string()]
                    .clone(),
            )
            .expect("the camera has a transform");

            assert_eq!((rig.height, rig.back, rig.zoom), (34.0, 13.7, 1.0));
            assert!((lens.fov.to_degrees() - 64.0).abs() < 1e-3);
            // Saved where its first frame puts it, so starting play does not
            // jump the view.
            let framed = rig.camera_transform(Vec3::zeros(), 0.0, 1.0);
            assert!((transform.transform.position - framed.position).magnitude() < 1e-4);
            assert!(transform.transform.rotation.angle_to(&framed.rotation) < 1e-4);
        }
    }

    /// The pixel `point` is drawn at by `camera` placed at `transform`.
    fn project(camera: &Camera, transform: &Transform, point: Vec3, viewport: Vec2) -> Vec2 {
        let clip =
            camera.projection * transform.inverse_matrix() * vec4(point.x, point.y, point.z, 1.0);
        let ndc = clip.xyz() / clip.w;
        vec2(
            (ndc.x + 1.0) * 0.5 * viewport.x,
            (1.0 - ndc.y) * 0.5 * viewport.y,
        )
    }

    #[test]
    fn the_cursor_ray_hits_the_ground_under_the_cursor_at_every_framing() {
        let rig = ComponentTankCamera::default();
        let viewport = vec2(1600.0, 900.0);
        let camera = far_camera(viewport.x / viewport.y);
        let tank = vec3(12.0, 0.0, -7.0);

        for zoom in [rig.min_zoom, 0.8, rig.max_zoom] {
            for lead in [Vec3::zeros(), vec3(6.0, 0.0, 0.0), vec3(-6.0, 0.0, 0.0)] {
                let transform = rig.camera_transform(tank + lead, 0.0, zoom);
                for offset in [
                    vec3(0.0, 0.0, 0.0),
                    vec3(9.0, 0.0, 4.0),
                    vec3(-11.0, 0.0, -5.0),
                ] {
                    let point = tank + lead + offset * zoom;
                    let pixel = project(&camera, &transform, point, viewport);
                    assert!(
                        pixel.x > 0.0
                            && pixel.x < viewport.x
                            && pixel.y > 0.0
                            && pixel.y < viewport.y
                    );

                    let ray = camera
                        .screen_ray(&transform, pixel, viewport)
                        .expect("cursor ray");
                    let hit = ground_intersection(&ray, 0.0).expect("cursor ray meets the ground");

                    assert!(
                        (hit - point).magnitude() < 1e-2,
                        "zoom {zoom} lead {lead:?}: {hit:?} != {point:?}"
                    );
                }
            }
        }
    }

    /// The arena, running headless with its player's tank, camera and
    /// crosshair.
    struct Arena {
        runner: calyx_engine::test_support::HeadlessSceneRunner,
        tank: GameObject,
        camera: GameObject,
        crosshair: GameObject,
    }

    /// The headless runner's screen.
    const SCREEN: Vec2 = Vec2::new(1280.0, 720.0);

    impl Arena {
        fn new() -> Self {
            let test_assets = calyx_engine::test_support::TestAssets::sandbox();
            let assets = test_assets.asset_context();
            let scene_ref = assets
                .registries
                .assets
                .read()
                .reload_by_path::<Scene>(
                    &test_assets.sandbox_root().join("game/scenes/arena.cxscene"),
                )
                .expect("the arena should load");
            let scene = scene_ref.read().clone();
            let named = |name: &str| {
                scene
                    .objects()
                    .find(|go| scene.name(*go) == name)
                    .unwrap_or_else(|| panic!("the arena should have a {name}"))
            };
            let (tank, camera, crosshair) = (named("Tank"), named("Camera"), named("Crosshair"));
            Self {
                runner: calyx_engine::test_support::HeadlessSceneRunner::from_scene(scene),
                tank,
                camera,
                crosshair,
            }
        }

        fn point_at(&mut self, pixel: Vec2) {
            self.runner.move_pointer(egui::pos2(pixel.x, pixel.y));
        }

        fn scroll(&mut self, points: f32) {
            self.runner.enqueue_event(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, points),
                modifiers: egui::Modifiers::default(),
            });
        }

        fn run(&mut self, seconds: f32) {
            self.runner.step_many((seconds * 60.0).round() as usize);
        }

        fn tank(&self) -> Vec3 {
            self.runner.scene().world_transform(self.tank).position
        }

        fn camera(&self) -> Transform {
            self.runner.scene().world_transform(self.camera)
        }

        /// How far the camera's focus leads the tank on the ground.
        fn lead(&self) -> Vec3 {
            let camera = self.camera();
            let ray = Ray {
                origin: camera.position,
                direction: camera.forward(),
            };
            let focus = ground_intersection(&ray, self.tank().y).expect("the camera looks down");
            vec3(focus.x - self.tank().x, 0.0, focus.z - self.tank().z)
        }

        /// The pixel the crosshair is drawn at, through the camera as it
        /// renders.
        fn crosshair_pixel(&self) -> Vec2 {
            let scene = self.runner.scene();
            let camera = scene
                .read_component::<ComponentCamera, _, _>(self.camera, |camera| {
                    camera.camera(SCREEN.x / SCREEN.y)
                })
                .expect("the camera has a lens");
            project(
                &camera,
                &self.camera(),
                scene.world_transform(self.crosshair).position,
                SCREEN,
            )
        }
    }

    #[test]
    fn aiming_to_either_side_leads_the_camera_that_way_and_keeps_the_crosshair_under_the_cursor() {
        for (cursor, side) in [(vec2(1180.0, 360.0), 1.0), (vec2(100.0, 360.0), -1.0)] {
            let mut arena = Arena::new();
            arena.point_at(cursor);
            arena.run(3.0);

            let lead = arena.lead();
            assert!(lead.x * side > 3.0, "lead {lead:?}");
            assert!(lead.magnitude() <= 6.0 + 1e-3, "lead {lead:?}");
            assert!(lead.z.abs() < 0.1, "lead {lead:?}");
            let pixel = arena.crosshair_pixel();
            assert!(
                (pixel - cursor).magnitude() < 0.5,
                "{pixel:?} != {cursor:?}"
            );
        }
    }

    #[test]
    fn aiming_at_the_tank_recentres_the_camera() {
        let mut arena = Arena::new();
        arena.point_at(vec2(1180.0, 360.0));
        arena.run(3.0);
        arena.point_at(SCREEN * 0.5);
        arena.run(4.0);

        assert!(arena.lead().magnitude() < 0.05, "lead {:?}", arena.lead());
    }

    #[test]
    fn scrolling_zooms_between_the_near_and_far_framings() {
        let mut arena = Arena::new();
        let cursor = vec2(900.0, 200.0);
        arena.point_at(cursor);
        arena.scroll(10_000.0);
        arena.run(2.0);

        let near = arena.camera().position.y - arena.tank().y;
        assert!((near - 20.4).abs() < 1e-2, "near height {near}");
        let pixel = arena.crosshair_pixel();
        assert!(
            (pixel - cursor).magnitude() < 0.5,
            "{pixel:?} != {cursor:?}"
        );

        arena.scroll(-10_000.0);
        arena.run(2.0);

        let far = arena.camera().position.y - arena.tank().y;
        assert!((far - 34.0).abs() < 1e-2, "far height {far}");
        let pixel = arena.crosshair_pixel();
        assert!(
            (pixel - cursor).magnitude() < 0.5,
            "{pixel:?} != {cursor:?}"
        );
    }
}
