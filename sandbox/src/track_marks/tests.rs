use super::*;
use calyx_engine::assets::AssetAccess;
use calyx_engine::component::ComponentID;
use calyx_engine::math::Transform;
use calyx_engine::test_support::{HeadlessSceneRunner, TestAssets};
use nalgebra::UnitQuaternion;

/// A field over a 40 by 40 m square centred on the origin, at 8 texels a
/// metre.
fn field() -> MarksField {
    MarksField::new([-20.0, -20.0, 40.0, 40.0], 320, 320)
}

fn pose(x: f32, z: f32, heading: f32) -> TreadPose {
    TreadPose::new(vec2(x, z), vec2(heading.sin(), heading.cos())).unwrap()
}

/// Drives `state` through `poses`, printing into `field` at full strength.
fn drive(
    field: &mut MarksField,
    state: &mut TreadState,
    poses: impl IntoIterator<Item = TreadPose>,
) {
    for pose in poses {
        for print in state.advance(pose, 1.0) {
            field.stamp(&print);
        }
    }
}

/// Poses 5 cm apart from `from` to `to` along Z, facing +Z.
fn straight(from: f32, to: f32) -> impl Iterator<Item = TreadPose> {
    let steps = ((to - from) / 0.05).round() as usize;
    (0..=steps).map(move |step| pose(0.0, from + step as f32 * 0.05, 0.0))
}

/// The strongest mark across the band of a tread centred at `point`, so the
/// grousers' weaker gaps do not matter.
fn band(field: &MarksField, point: Vec2, along: Vec2) -> u8 {
    (-5..=5)
        .map(|step| field.intensity_at(point + along * (step as f32 * 0.05)))
        .max()
        .unwrap()
}

#[test]
fn a_straight_drive_prints_two_parallel_bands_and_nothing_between_or_outside() {
    let mut field = field();
    let mut state = TreadState::new(pose(0.0, 0.0, 0.0));
    drive(&mut field, &mut state, straight(0.0, 10.0));

    let forward = vec2(0.0, 1.0);
    for z in [1.0, 4.0, 8.0] {
        for x in [-TREAD_OFFSET, TREAD_OFFSET] {
            assert!(
                band(&field, vec2(x, z), forward) > 200,
                "the tread at x {x} prints at z {z}"
            );
        }
        assert_eq!(
            field.intensity_at(vec2(0.0, z)),
            0,
            "nothing between the treads"
        );
        let outside = TREAD_OFFSET + TREAD_WIDTH;
        assert_eq!(
            field.intensity_at(vec2(outside, z)),
            0,
            "nothing outside the treads"
        );
        assert_eq!(
            field.intensity_at(vec2(-outside, z)),
            0,
            "nothing outside the treads"
        );
    }
    assert_eq!(
        field.intensity_at(vec2(TREAD_OFFSET, -2.0)),
        0,
        "nothing behind where the tank started"
    );
}

#[test]
fn the_grousers_stay_put_on_the_ground_as_the_tread_rolls_over_them() {
    let mut once = field();
    let mut state = TreadState::new(pose(0.0, 0.0, 0.0));
    drive(&mut once, &mut state, straight(0.0, 0.05));
    let mut rolled = field();
    let mut state = TreadState::new(pose(0.0, 0.0, 0.0));
    drive(&mut rolled, &mut state, straight(0.0, 0.6));

    // A point printed by the first frame keeps its strength however far the
    // tread rolls on over it.
    let point = vec2(TREAD_OFFSET, 0.2);
    assert_eq!(once.intensity_at(point), rolled.intensity_at(point));
    let strengths = (0..20)
        .map(|step| rolled.intensity_at(vec2(TREAD_OFFSET, step as f32 * 0.05)))
        .collect::<Vec<_>>();
    let (low, high) = (
        *strengths.iter().min().unwrap(),
        *strengths.iter().max().unwrap(),
    );
    assert!(high - low > 30, "the grousers show: {strengths:?}");
    assert!(low > 150, "but the band stays whole: {strengths:?}");
}

#[test]
fn a_curving_drive_prints_bands_that_follow_the_arc() {
    let mut field = field();
    // Round a circle of 8 m radius about (8, 0), heading along it.
    let radius = 8.0;
    let poses = (0..=300).map(|step| {
        let angle = step as f32 * 0.005;
        pose(radius - radius * angle.cos(), radius * angle.sin(), angle)
    });
    let mut state = TreadState::new(pose(0.0, 0.0, 0.0));
    drive(&mut field, &mut state, poses);

    for angle in [0.4f32, 0.8, 1.2] {
        let centre = vec2(radius, 0.0);
        let outward = vec2(-angle.cos(), angle.sin());
        let along = vec2(outward.y, -outward.x);
        for r in [radius - TREAD_OFFSET, radius + TREAD_OFFSET] {
            assert!(
                band(&field, centre + outward * r, along) > 200,
                "a tread prints at radius {r}, angle {angle}"
            );
        }
        assert_eq!(field.intensity_at(centre + outward * radius), 0);
    }
}

#[test]
fn turning_in_place_sweeps_the_treads_round_into_scuffs() {
    let mut field = field();
    let mut state = TreadState::new(pose(0.0, 0.0, 0.0));
    let turn =
        (0..=60).map(|step| pose(0.0, 0.0, step as f32 * std::f32::consts::FRAC_PI_2 / 60.0));
    drive(&mut field, &mut state, turn);

    // A quarter turn leaves the treads across where they started, and marks
    // the ground the ends of the treads swept between.
    for x in [-TREAD_OFFSET, TREAD_OFFSET] {
        assert!(band(&field, vec2(0.0, x), vec2(1.0, 0.0)) > 200);
    }
    // Halfway round, a tread's centre line ran through this point.
    let diagonal = std::f32::consts::FRAC_1_SQRT_2 * TREAD_OFFSET;
    assert!(
        field.intensity_at(vec2(diagonal, -diagonal)) > 100,
        "the swept ground"
    );
    assert!(
        field.intensity_at(vec2(-diagonal, diagonal)) > 100,
        "the swept ground"
    );
    assert_eq!(field.intensity_at(vec2(0.0, 0.0)), 0, "the hull's centre");
}

#[test]
fn standing_still_or_jumping_to_a_respawn_prints_nothing() {
    let mut state = TreadState::new(pose(0.0, 0.0, 0.0));
    assert!(state.advance(pose(0.0, 0.001, 0.0), 1.0).is_empty());
    assert!(state.advance(pose(10.0, 10.0, 1.0), 1.0).is_empty());
    assert_eq!(state.advance(pose(10.0, 10.1, 1.0), 1.0).len(), 2);
}

#[test]
fn marks_fade_out_over_the_fade_time_and_their_tiles_go_quiet() {
    let mut field = field();
    let mut state = TreadState::new(pose(0.0, 0.0, 0.0));
    drive(&mut field, &mut state, straight(0.0, 10.0));
    let point = vec2(TREAD_OFFSET, 5.0);
    let fresh = band(&field, point, vec2(0.0, 1.0));
    assert!(!field.take_dirty().is_empty(), "the prints upload");
    assert!(field.take_dirty().is_empty(), "each change uploads once");
    let tiles = field.active_tiles();
    assert!(tiles > 0);

    // Halfway through a 20 s fade, at 60 frames a second.
    let fade_time = 20.0;
    let dt = 1.0 / 60.0;
    for _ in 0..600 {
        field.advance(dt, fade_time);
    }
    let half = band(&field, point, vec2(0.0, 1.0));
    assert!(
        (half as f32 - (fresh as f32 - 127.5)).abs() < 8.0,
        "half faded: {fresh} -> {half}"
    );
    assert!(!field.take_dirty().is_empty(), "the fade uploads");
    // A step uploads only the tiles due one, not every active tile.
    field.advance(dt, fade_time);
    assert!(field.take_dirty().len() < tiles);

    for _ in 0..601 {
        field.advance(dt, fade_time);
    }
    assert_eq!(
        band(&field, point, vec2(0.0, 1.0)),
        0,
        "gone after the fade time"
    );
    assert_eq!(field.active_tiles(), 0);
    field.take_dirty();
    field.advance(dt, fade_time);
    assert!(field.take_dirty().is_empty(), "a faded field costs nothing");
}

#[test]
fn driving_on_over_old_ground_does_not_grow_the_work() {
    let mut field = field();
    let mut state = TreadState::new(pose(0.0, 0.0, 0.0));
    let dt = 1.0 / 60.0;
    let lap = |field: &mut MarksField, state: &mut TreadState| {
        let poses = straight(-10.0, 10.0)
            .chain(straight(-10.0, 10.0).collect::<Vec<_>>().into_iter().rev());
        for pose in poses {
            for print in state.advance(pose, 1.0) {
                field.stamp(&print);
            }
            field.advance(dt, 25.0);
        }
    };
    lap(&mut field, &mut state);
    let tiles = field.active_tiles();
    for _ in 0..20 {
        lap(&mut field, &mut state);
    }
    assert_eq!(field.active_tiles(), tiles);
}

#[test]
fn a_full_mark_darkens_by_the_tint_and_no_mark_leaves_the_ground_white() {
    let table = encode_table([0.2, 0.3, 0.4]);
    assert_eq!(table[0], [255; 4]);
    assert_eq!(table[255], [204, 179, 153, 255]);
}

/// The sandbox's Sunny ground material and flat heightmap.
const GROUND_MATERIAL: &str = "aee596fc-b294-0364-b6f9-5f40f4976543";
const HEIGHTMAP: &str = "bbdac185-6020-2ced-2478-957277f4faf3";

fn tank(scene: &mut Scene, x: f32, y: f32, z: f32) -> GameObject {
    let tank = scene.create(
        Some(ComponentID {
            name: "Tank".into(),
            ..Default::default()
        }),
        None,
    );
    scene.set_world_transform(tank, Transform::from_xyz(x, y, z).matrix());
    scene.add_component(tank, ComponentTankController::default());
    tank
}

fn place(scene: &mut Scene, tank: GameObject, x: f32, y: f32, z: f32, heading: f32) {
    scene.set_world_transform(
        tank,
        Transform::from_components(
            nalgebra_glm::vec3(x, y, z),
            UnitQuaternion::from_euler_angles(0.0, heading, 0.0),
            Vec3::repeat(1.0),
        )
        .matrix(),
    );
}

#[test]
fn tanks_on_a_terrain_print_into_a_copy_of_its_material_and_tanks_above_it_do_not() {
    let assets = TestAssets::game();
    let context = assets.asset_context();
    let registries = context.registries.lock_read();
    let mut scene = registries.scene();
    let terrain = scene.create(None, None);
    // The arena's ground: flat at Y 0, 200 m square.
    scene.set_world_transform(terrain, Transform::from_xyz(0.0, -1.3, 0.0).matrix());
    let original = AssetRef::<Material>::from_id(GROUND_MATERIAL.parse().unwrap());
    scene.add_component(
        terrain,
        ComponentTerrain {
            heightmap: AssetRef::from_id(HEIGHTMAP.parse().unwrap()),
            material: original.clone(),
            size: vec2(200.0, 200.0),
            height_scale: 1.3,
            cast_shadows: false,
        },
    );
    let ground = tank(&mut scene, 30.0, 0.0, 30.0);
    let deck = tank(&mut scene, 40.0, 2.0, 30.0);

    let mut runner = HeadlessSceneRunner::from_scene(scene);
    runner.set_step_seconds(1.0 / 60.0);
    assert!(
        runner
            .scene()
            .read_component::<ComponentTrackMarks, _, _>(terrain, |_| ())
            .is_some(),
        "the tanks give the terrain its marks when play starts"
    );
    for step in 0..=40 {
        let z = 30.0 + step as f32 * 0.1;
        place(runner.scene_mut(), ground, 30.0, 0.0, z, 0.0);
        place(runner.scene_mut(), deck, 40.0, 2.0, z, 0.0);
        runner.step();
    }

    let scene = runner.scene();
    let material = scene
        .read_component::<ComponentTerrain, _, _>(terrain, |terrain| terrain.material.clone())
        .unwrap();
    assert_ne!(material.id(), original.id(), "the terrain draws a copy");
    let copy = material.get_ref(&registries).unwrap();
    let copy = copy.read();
    let variable = |name: &str| {
        copy.variables
            .iter()
            .find(|variable| variable.name == name)
            .map(|variable| variable.value.clone())
            .unwrap()
    };
    assert!(matches!(
        variable("marks_rect"),
        ShaderVariableValue::Vec4([-100.0, -100.0, 200.0, 200.0])
    ));
    let original = original.get_ref(&registries).unwrap();
    assert!(
        original
            .read()
            .variables
            .iter()
            .any(|variable| variable.name == "marks_rect"
                && matches!(variable.value, ShaderVariableValue::Vec4([_, _, w, _]) if w == 0.0)),
        "the material asset itself is left as it was"
    );

    scene
        .read_component::<ComponentTrackMarks, _, _>(terrain, |marks| {
            let runtime = marks.runtime.as_ref().expect("the marks run");
            let ShaderVariableValue::Texture2D(MaterialTexture::Asset(texture)) =
                variable("marks_texture")
            else {
                panic!("the copy samples a texture");
            };
            assert_eq!(texture.id(), runtime.texture.id());
            let forward = vec2(0.0, 1.0);
            assert!(band(&runtime.field, vec2(30.0 + TREAD_OFFSET, 32.0), forward) > 200);
            assert_eq!(
                band(&runtime.field, vec2(40.0 + TREAD_OFFSET, 32.0), forward),
                0,
                "a tank up on a deck prints nothing"
            );
        })
        .unwrap();
}
