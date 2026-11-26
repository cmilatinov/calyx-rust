use super::*;
use calyx_engine::scene::SceneHud;
use calyx_engine::ui::{UiInput, UiPoint, UiSize};
fn setup() -> (Scene, GameObject) {
    let mut scene = calyx_engine::test_support::test_scene();
    let tank = scene.create(None, None);
    scene.add_component(
        tank,
        ComponentTankController {
            ammo: 2,
            max_ammo: 6,
            reload_remaining: 0.8,
            ..Default::default()
        },
    );
    scene.add_component(
        tank,
        ComponentHealth {
            current_health: 42.0,
            max_health: 90.0,
            ..Default::default()
        },
    );
    scene.add_component(tank, ComponentTankHud::default());
    (scene, tank)
}

fn commands(scene: &Scene, width: f32, height: f32) -> Vec<PaintCommand> {
    SceneHud::default()
        .frame(
            scene,
            UiRect::from_min_size(UiPoint::new(30.0, 50.0), UiSize::new(width, height)),
            UiInput::default(),
        )
        .unwrap()
        .paint_commands
}

#[test]
fn timer_and_radar_remain_without_removed_bottom_instrument_panels() {
    let (scene, _) = setup();
    for (width, height) in [(1672.0, 941.0), (960.0, 540.0)] {
        let commands = commands(&scene, width, height);
        let text = commands
            .iter()
            .filter_map(|c| {
                if let PaintCommand::Label(label) = c {
                    Some(label.text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert!(text.contains(&"05:00"));
        assert!(text.contains(&"N"));
        assert!(!text
            .iter()
            .any(|s| s.contains("HULL") || s.contains("CANNON") || s.contains("RELOAD")));
    }
}

#[test]
fn health_bar_width_is_fixed_and_anchor_clears_projected_top() {
    let far = hp_anchor(ScreenBounds {
        left: 100.0,
        right: 180.0,
        top: 200.0,
    });
    let near = hp_anchor(ScreenBounds {
        left: 100.0,
        right: 260.0,
        top: 200.0,
    });
    let tiny = hp_anchor(ScreenBounds {
        left: 0.0,
        right: 10.0,
        top: 80.0,
    });
    assert_eq!(far, (140.0, 192.0, HP_BAR_WIDTH));
    assert_eq!(near, (180.0, 192.0, HP_BAR_WIDTH));
    assert_eq!(tiny, (5.0, 72.0, HP_BAR_WIDTH));
}

/// The outer frame of the one world health bar in a HUD frame: a black
/// fill directly followed by its panel fill and its friendly cyan fill.
fn health_bar_frame(commands: &[PaintCommand]) -> UiRect {
    let frames = commands
        .windows(3)
        .filter_map(|window| match window {
            [PaintCommand::FillRect {
                rect, color: BLACK, ..
            }, PaintCommand::FillRect {
                color: paint::PANEL,
                ..
            }, PaintCommand::FillRect { color: CYAN, .. }] => Some(*rect),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(frames.len(), 1, "expected one health bar");
    frames[0]
}

#[test]
fn turning_a_tank_in_place_keeps_its_health_bar_size() {
    use calyx_engine::assets::{mesh::Mesh, AssetRef};
    use calyx_engine::component::{ComponentCamera, ComponentMesh};
    use calyx_engine::math::Transform;
    use nalgebra::UnitQuaternion;
    use nalgebra_glm::vec3;

    let assets = calyx_engine::test_support::test_asset_context_with_assets(Vec::new());
    let mut hull = Mesh::new(&assets.render_context);
    // A tank-shaped hull: longer than it is wide, so its projected width
    // changes as it turns.
    hull.vertices = vec![vec3(-1.1, 0.0, -1.7), vec3(1.1, 1.3, 1.7)];
    let hull = assets
        .registries
        .assets
        .read()
        .create("hud-tank-hull".into(), hull)
        .unwrap();
    let mut scene = assets.registries.lock_read().scene();
    let tank = scene.create(None, None);
    scene.add_component(tank, ComponentTankController::default());
    scene.add_component(
        tank,
        ComponentHealth {
            current_health: 60.0,
            max_health: 100.0,
            ..Default::default()
        },
    );
    scene.add_component(tank, ComponentTankHud::default());
    scene.add_component(
        tank,
        ComponentMesh {
            mesh: AssetRef::from_id(hull.id()),
            ..Default::default()
        },
    );
    // The arena's chase camera, relative to the tank.
    let camera = scene.create(None, None);
    scene.add_component(
        camera,
        ComponentCamera {
            fov: 1.117,
            ..Default::default()
        },
    );
    scene.set_transform(
        camera,
        &Transform::from_components(
            vec3(0.0, 34.0, -13.7),
            UnitQuaternion::from_euler_angles(68.05f32.to_radians(), 0.0, 0.0),
            Vec3::repeat(1.0),
        )
        .matrix(),
    );
    let (width, height) = (1326.0, 746.0);
    let projection = Projection::scene(&scene, width, height).unwrap();
    let mut bounds = Vec::new();
    let mut frames = Vec::new();
    for degrees in [0.0f32, 40.0, 90.0, 135.0] {
        scene.set_transform(
            tank,
            &Transform::from_components(
                Vec3::zeros(),
                UnitQuaternion::from_euler_angles(0.0, degrees.to_radians(), 0.0),
                Vec3::repeat(1.0),
            )
            .matrix(),
        );
        let projected = projection.visual_bounds(&scene, tank).unwrap();
        bounds.push(projected.right - projected.left);
        frames.push(health_bar_frame(&commands(&scene, width, height)));
    }
    // The projected bounds really do change as the tank turns ...
    let (narrow, wide) = bounds.iter().fold((f32::INFINITY, 0.0f32), |(lo, hi), w| {
        (lo.min(*w), hi.max(*w))
    });
    assert!(wide > narrow * 1.2, "bounds widths {bounds:?}");
    // ... but the bar keeps one width and thickness.
    for frame in &frames {
        assert!(
            (frame.width() - (HP_BAR_WIDTH + 6.0)).abs() < 1e-3,
            "{frames:?}"
        );
        assert!(
            (frame.height() - (HP_BAR_THICKNESS + 6.0)).abs() < 1e-3,
            "{frames:?}"
        );
    }
}

#[test]
fn timer_uses_simulation_delta_resets_with_scene_and_reads_match_state() {
    let (scene, _) = setup();
    let mut hud = ComponentTankHud::default();
    hud.advance(1.2);
    assert_eq!(format_time(hud.time(&scene)), "04:59");
    hud.advance(0.0);
    assert!((hud.time(&scene) - 298.8).abs() < 0.001);
    hud.advance(-1.0);
    hud.advance(f32::NAN);
    assert!((hud.time(&scene) - 298.8).abs() < 0.001);
    let restored: ComponentTankHud =
        serde_json::from_value(serde_json::to_value(&hud).unwrap()).unwrap();
    assert_eq!(restored.elapsed, 0.0);
    hud.timer_duration = 0.0;
    assert_eq!(format_time(hud.time(&scene)), "00:01");
    let mut scene = scene;
    let state = scene.create(None, None);
    scene.add_component(
        state,
        ComponentGameState {
            match_time_remaining: 123.0,
            ..Default::default()
        },
    );
    assert_eq!(format_time(hud.time(&scene)), "02:03");
    scene.write_component::<ComponentGameState, _>(state, |state| {
        state.match_duration = 0.0;
        state.phase_elapsed = 42.0;
    });
    assert_eq!(format_time(hud.time(&scene)), "00:42");
}

#[test]
fn damage_tiers_are_removed_health_ratios() {
    assert_eq!(damage_tier(10.0, 100.0, 0.15, 0.5), DamageTier::Low);
    assert_eq!(damage_tier(35.0, 100.0, 0.15, 0.5), DamageTier::Normal);
    assert_eq!(damage_tier(60.0, 100.0, 0.15, 0.5), DamageTier::High);
}

#[test]
fn scoreboard_is_held_and_bounded_with_many_real_rows() {
    let (mut scene, tank) = setup();
    for i in 0..80 {
        let actor = scene.create(None, None);
        scene.add_component(actor, ComponentTankController::default());
        scene.add_component(
            actor,
            ComponentCombatStats {
                team: i % 2,
                ..Default::default()
            },
        );
    }
    assert!(!commands(&scene, 960.0, 540.0)
        .iter()
        .any(|c| matches!(c,PaintCommand::Label(label)if label.text=="SCOREBOARD")));
    scene.write_component::<ComponentTankHud, _>(tank, |hud| hud.scoreboard_open = true);
    let commands = commands(&scene, 960.0, 540.0);
    assert!(commands
        .iter()
        .any(|c| matches!(c,PaintCommand::Label(label)if label.text=="SCOREBOARD")));
    assert!(commands
        .iter()
        .any(|c| matches!(c,PaintCommand::Label(label)if label.text.starts_with("PAGE 1/"))));
    for command in commands {
        if let PaintCommand::Label(label) = command {
            assert!(label.position.y <= 590.0, "text overflow {label:?}");
        }
    }
}

#[test]
fn a_finished_free_for_all_shows_one_ranked_list_and_the_result() {
    use crate::match_rules::{update_match_rules, ComponentMatchRules, MATCH_MODE_FREE_FOR_ALL};
    let (mut scene, tank) = setup();
    scene.add_component(
        tank,
        ComponentCombatStats {
            kills: 4,
            ..Default::default()
        },
    );
    // Every tank is unassigned, the same team, yet each plays for itself.
    for kills in 0..14 {
        let actor = scene.create(
            Some(ComponentID {
                name: format!("Bot {kills}"),
                ..Default::default()
            }),
            None,
        );
        scene.add_component(actor, ComponentTankController::default());
        scene.add_component(
            actor,
            ComponentCombatStats {
                kills: kills % 4,
                ..Default::default()
            },
        );
    }
    let rules = scene.create(None, None);
    scene.add_component(
        rules,
        ComponentMatchRules {
            mode: MATCH_MODE_FREE_FOR_ALL,
            time_limit: 60.0,
            ..Default::default()
        },
    );
    let playing = commands(&scene, 960.0, 540.0);
    assert!(playing
        .iter()
        .any(|c| matches!(c, PaintCommand::Label(label) if label.text == "01:00")));
    assert!(update_match_rules(&mut scene, rules, 60.0).is_some());

    // No key is held: the final scoreboard opens by itself.
    let commands = commands(&scene, 960.0, 540.0);
    let labels = commands
        .iter()
        .filter_map(|c| match c {
            PaintCommand::Label(label) => Some(label),
            _ => None,
        })
        .collect::<Vec<_>>();
    let text = labels.iter().map(|l| l.text.as_str()).collect::<Vec<_>>();
    for expected in [
        "SCOREBOARD",
        "VICTORY",
        "TIME LIMIT REACHED",
        "PAGE 1/2  /  SCROLL",
    ] {
        assert!(text.contains(&expected), "{expected} in {text:?}");
    }
    assert!(!text.contains(&"01:00"), "gameplay overlays hide");
    // One list ranks the twelve leaders, the player first in its colour.
    for rank in 1..=12 {
        assert!(text.contains(&rank.to_string().as_str()), "rank {rank}");
    }
    assert!(!text.contains(&"No combatants"));
    let first_bot = labels
        .iter()
        .find(|l| l.text.starts_with("Bot "))
        .expect("bots are listed");
    assert_eq!(first_bot.color, RED);
    for label in labels {
        assert!(label.position.y <= 590.0, "text overflow {label:?}");
    }
}

#[test]
fn scoreboard_blur_is_local_reused_and_removed_without_touching_world_effects() {
    let (mut scene, owner) = setup();
    let camera = scene.create(None, None);
    scene.add_component(camera, calyx_engine::component::ComponentCamera::default());
    let world = scene.create(None, None);
    scene.add_component(
        world,
        calyx_engine::component::ComponentGlobalPostProcessVolume::default(),
    );
    let authored = ComponentOutline {
        styles: vec![calyx_engine::component::OutlineStyle {
            opacity: 0.85,
            ..Default::default()
        }],
        ..Default::default()
    };
    scene.add_component(world, authored.clone());
    let mut settings = calyx_engine::render::postprocess::PostProcessSettings::default();
    let before = scene.objects().count();
    let blur = update_scoreboard_blur(&mut scene, owner, true, Default::default());
    let object = blur.game_object(&scene).unwrap();
    assert_eq!(scene.parent(object), Some(owner));
    assert_eq!(scene.objects().count(), before + 1);
    assert_eq!(
        scene.read_component::<ComponentLocalPostProcessVolume, _, _>(object, |volume| volume
            .weight),
        Some(1.0)
    );
    settings.resolve_into(&scene, &Vec3::zeros());
    assert_eq!(
        settings.get::<ComponentOutline>().unwrap().styles[0].opacity,
        0.0
    );
    assert_eq!(
        settings.get::<ComponentDepthOfField>().unwrap().max_blur,
        30.0
    );
    let reused = update_scoreboard_blur(&mut scene, owner, true, blur);
    assert_eq!(reused.game_object(&scene), Some(object));
    update_scoreboard_blur(&mut scene, owner, false, blur);
    scene.flush_deletes();
    assert_eq!(scene.objects().count(), before);
    settings.resolve_into(&scene, &Vec3::zeros());
    assert_eq!(settings.get::<ComponentOutline>(), Some(&authored));
    assert!(settings.get::<ComponentDepthOfField>().is_none());
    let blur = update_scoreboard_blur(&mut scene, owner, true, Default::default());
    scene.delete(owner);
    scene.flush_deletes();
    assert!(blur.game_object(&scene).is_none());
}
