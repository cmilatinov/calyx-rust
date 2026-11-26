use super::*;
use egui::{Event, RawInput};

#[test]
fn editor_camera_map_flies_with_keys_and_modifiers() {
    let ctx = egui::Context::default();
    let mut state = InputState::new(EditorCamera::action_map());
    let key = |key| Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    ctx.begin_pass(RawInput {
        modifiers: Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        },
        events: vec![key(Key::W), key(Key::A)],
        ..Default::default()
    });
    let input = Input::from_ctx(&ctx, None, &mut state, true);

    assert_eq!(input.axis(MOVE_FORWARD), 1.0);
    assert_eq!(input.axis(MOVE_RIGHT), -1.0);
    assert_eq!(input.axis(MOVE_UP), -1.0);
    assert!(input.action(BOOST).pressed());
    let _ = ctx.end_pass();
}

#[test]
fn look_at_faces_the_target_and_flying_turns_from_there() {
    let mut camera = EditorCamera::default();
    camera.transform.position = Vec3::new(0.0, 2.0, 0.0);
    assert!(camera.look_at(&Vec3::new(3.0, 2.0, 4.0)));
    let forward = camera.transform.forward();
    assert!(
        (forward - Vec3::new(0.6, 0.0, 0.8)).magnitude() < 1e-5,
        "{forward}"
    );
    // The angles flying starts from match the new rotation.
    let rebuilt = UnitQuaternion::from_euler_angles(camera.rotation.x, camera.rotation.y, 0.0);
    assert!(rebuilt.angle_to(&camera.transform.rotation) < 1e-4);

    let before = camera.transform;
    assert!(!camera.look_at(&Vec3::new(0.0, 2.0, 0.0)));
    assert_eq!(camera.transform.rotation, before.rotation);
}

#[test]
fn look_at_straight_down_or_up_stays_upright_at_the_flying_pitch_limit() {
    for (target, pitch) in [(-10.0, 89.0f32), (10.0, -89.0)] {
        let mut camera = EditorCamera::default();
        camera.transform.position = Vec3::zeros();
        assert!(camera.look_at(&Vec3::new(0.0, target, 0.0)));
        let forward = camera.transform.forward();
        assert!(forward.iter().all(|v| v.is_finite()), "{forward}");
        assert!(
            (forward.y.abs() - 89f32.to_radians().sin()).abs() < 1e-4,
            "{forward}"
        );
        assert_eq!(forward.y.signum(), target.signum());
        assert!((camera.rotation.x - pitch.to_radians()).abs() < 1e-5);
        assert!(camera.transform.right().y.abs() < 1e-5, "no roll");
    }
}
