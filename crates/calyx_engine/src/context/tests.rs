use super::{GameContext, UpdatedScene};
use crate::component::{
    ComponentID, ComponentParticleSystem, ParticleEmission, ParticleForces, ParticleScalarRange,
    ParticleVelocity,
};
use crate::input::Input;
use crate::math::{CurveKey, GradientKey, Transform};
use crate::render::{Camera, SceneRenderer, SceneRendererOptions};
use crate::scene::GameObject;
use crate::test_support::TestAssets;
use egui::Color32;
use nalgebra_glm::Vec3;
use std::collections::HashSet;
use uuid::Uuid;

/// Particles a second the test emitter emits.
const RATE: f32 = 10.0;

/// A system emitting [`RATE`] still particles a second, each living ten
/// seconds.
fn steady_emitter() -> ComponentParticleSystem {
    let mut system = ComponentParticleSystem::default();
    system.emission = ParticleEmission {
        rate: RATE,
        burst_count: 0,
        max_particles: 1000,
        ..Default::default()
    };
    system.lifetime = ParticleScalarRange {
        min: 10.0,
        max: 10.0,
    };
    system.velocity = ParticleVelocity {
        initial: Vec3::zeros(),
        randomness: Vec3::zeros(),
    };
    system.forces = ParticleForces {
        acceleration: Vec3::zeros(),
    };
    system
}

/// A game context over the engine assets whose authoring scene holds one
/// steady emitter, and that emitter's object.
fn game_with_emitter(assets: &TestAssets) -> (GameContext, GameObject) {
    let mut game = GameContext::new(assets.asset_context());
    let scene = game.scenes.current_scene_mut();
    let emitter = scene.create(
        Some(ComponentID {
            id: Uuid::new_v4(),
            name: "Emitter".into(),
            visible: true,
        }),
        None,
    );
    scene.add_component(emitter, steady_emitter());
    scene.set_transform(emitter, &Transform::default().matrix());
    (game, emitter)
}

/// Runs one editor frame of `delta_time` seconds.
fn editor_frame(game: &mut GameContext, preview_particles: bool, delta_time: f64) {
    game.resources.time_mut().advance_by(delta_time);
    let context = egui::Context::default();
    let input = Input::from_ctx(&context, None, game.resources.input_mut(), false);
    game.update_scene(UpdatedScene::Editor { preview_particles }, &input);
}

/// The game objects whose particles `renderer` draws after rendering the
/// scene the editor shows.
fn drawn_systems(game: &GameContext, renderer: &mut SceneRenderer) -> HashSet<GameObject> {
    let camera = Camera::new(1.0, 60f32.to_radians(), 0.1, 200.0);
    let transform = Transform::from_xyz(0.0, 0.0, 5.0);
    renderer.render_scene_base(
        &camera,
        &transform,
        game.scenes.simulation_scene(),
        Some(&game.particles),
        None,
    );
    renderer.particle_renderer().tracked_systems()
}

fn renderer(game: &GameContext) -> SceneRenderer {
    SceneRenderer::new(
        &game.assets.lock_read(),
        SceneRendererOptions {
            clear_color: Color32::BLACK,
            ..Default::default()
        },
        (64, 64),
    )
    .expect("the renderer builds")
}

#[test]
fn the_edit_scene_previews_its_particles_while_not_playing() {
    let assets = TestAssets::engine();
    let (mut game, emitter) = game_with_emitter(&assets);
    let mut renderer = renderer(&game);

    for _ in 0..4 {
        editor_frame(&mut game, true, 0.5);
    }

    assert_eq!(
        game.particles.scene_id(),
        Some(game.scenes.current_scene().id())
    );
    assert_eq!(game.particles.frames(), 4);
    // Four half-second frames at ten particles a second.
    assert_eq!(game.particles.live_count(emitter), Some(20));
    assert_eq!(
        drawn_systems(&game, &mut renderer),
        HashSet::from([emitter])
    );
}

#[test]
fn turning_the_preview_off_stops_and_hides_the_edit_scene_particles() {
    let assets = TestAssets::engine();
    let (mut game, emitter) = game_with_emitter(&assets);
    let mut renderer = renderer(&game);
    editor_frame(&mut game, true, 0.5);
    assert_eq!(
        drawn_systems(&game, &mut renderer),
        HashSet::from([emitter])
    );

    editor_frame(&mut game, false, 0.5);
    editor_frame(&mut game, false, 0.5);

    assert_eq!(game.particles.scene_id(), None);
    assert_eq!(game.particles.live_count(emitter), None);
    assert!(drawn_systems(&game, &mut renderer).is_empty());

    // Back on, the preview starts over.
    editor_frame(&mut game, true, 0.5);
    assert_eq!(game.particles.frames(), 1);
    assert_eq!(game.particles.live_count(emitter), Some(5));
}

#[test]
fn play_starts_from_empty_particles_and_stop_returns_to_a_fresh_preview() {
    let assets = TestAssets::engine();
    let (mut game, emitter) = game_with_emitter(&assets);
    let mut renderer = renderer(&game);
    for _ in 0..4 {
        editor_frame(&mut game, true, 0.5);
    }
    assert_eq!(game.particles.live_count(emitter), Some(20));

    // Play: the copy's systems start empty, not from the preview.
    game.scenes.start_simulation();
    editor_frame(&mut game, true, 0.1);
    let play = game.scenes.simulation_scene().id();
    assert_eq!(game.particles.scene_id(), Some(play));
    assert_eq!(game.particles.frames(), 1);
    let played: HashSet<GameObject> = game.particles.systems().collect();
    assert_eq!(played.len(), 1);
    assert!(!played.contains(&emitter));
    let copy = *played.iter().next().unwrap();
    assert_eq!(game.particles.live_count(copy), Some(1));
    assert_eq!(drawn_systems(&game, &mut renderer), played);

    // Paused play stays frozen, preview or not.
    game.scenes.pause_simulation();
    editor_frame(&mut game, true, 0.5);
    assert_eq!(game.particles.scene_id(), Some(play));
    assert_eq!(game.particles.frames(), 1);
    assert_eq!(game.particles.live_count(copy), Some(1));
    game.scenes.start_simulation();
    editor_frame(&mut game, true, 0.1);
    assert_eq!(game.particles.frames(), 2);

    // Stop: the edit scene previews again, from fresh systems.
    game.scenes.stop_simulation();
    editor_frame(&mut game, true, 0.5);
    assert_eq!(
        game.particles.scene_id(),
        Some(game.scenes.current_scene().id())
    );
    assert_eq!(game.particles.frames(), 1);
    assert_eq!(game.particles.live_count(emitter), Some(5));
    assert_eq!(
        drawn_systems(&game, &mut renderer),
        HashSet::from([emitter])
    );
}

#[test]
fn editing_the_edit_scene_in_place_keeps_the_preview_running() {
    let assets = TestAssets::engine();
    let (mut game, emitter) = game_with_emitter(&assets);
    editor_frame(&mut game, true, 0.5);
    editor_frame(&mut game, true, 0.5);
    assert_eq!(game.particles.live_count(emitter), Some(10));

    // An inspector edit and a gizmo move write the scene in place.
    let scene = game.scenes.current_scene_mut();
    scene.write_component::<ComponentParticleSystem, _>(emitter, |system| {
        system.emission.rate = 2.0 * RATE;
    });
    scene.set_transform(emitter, &Transform::from_xyz(1.0, 2.0, 3.0).matrix());
    editor_frame(&mut game, true, 0.5);

    assert_eq!(game.particles.frames(), 3);
    // Ten from before the edits, then ten in half a second at twice the rate.
    assert_eq!(game.particles.live_count(emitter), Some(20));
}

#[test]
fn preparing_the_editor_uploads_edit_scene_curve_and_gradient_edits_to_the_preview() {
    let assets = TestAssets::engine();
    let (mut game, emitter) = game_with_emitter(&assets);
    editor_frame(&mut game, true, 0.5);
    assert_eq!(game.particles.system(emitter).unwrap().key_uploads(), 1);

    // An inspector edit of the edit scene's color gradient and size curve.
    let (gradient, curve) = game
        .scenes
        .current_scene_mut()
        .write_component::<ComponentParticleSystem, _>(emitter, |system| {
            system
                .color
                .gradient
                .insert(GradientKey::rgba(0.5, [0.0, 1.0, 0.0, 0.5]));
            system.size.curve.insert(CurveKey::new(0.5, 3.0));
            (system.color.gradient.clone(), system.size.curve.clone())
        })
        .expect("the emitter has a particle system");

    // The editor prepares before it steps, so the preview's views draw the
    // edit even on a frame that advances no particles.
    game.prepare_scene(UpdatedScene::Editor {
        preview_particles: true,
    });

    assert_eq!(game.particles.frames(), 1, "prepare advances no time");
    let system = game.particles.system(emitter).unwrap();
    assert_eq!(system.key_uploads(), 3, "both edits are uploaded");
    let (curve_range, gradient_range) = system.key_ranges();
    assert_eq!(gradient_range.count, gradient.keys.len() as u32);
    assert_eq!(curve_range.count, curve.keys.len() as u32);
}
