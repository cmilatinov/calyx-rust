use super::{Animation, AnimationKeyFrames, QuatKeyFrame, VectorKeyFrame};
use crate::assets::error::AssetError;
use crate::assets::Asset;
use crate::test_support::test_game_context;
use nalgebra::Unit;
use nalgebra_glm::{Quat, Vec3};
use std::collections::HashMap;
use std::fs;
use uuid::Uuid;

#[test]
fn from_file_returns_error_for_missing_file() {
    let context = test_game_context().assets.lock_read();
    let path = std::env::temp_dir().join(format!("missing-{}.cxanimclip", Uuid::new_v4()));
    let result = Animation::from_file(&context, &path);
    let Err(err) = result else {
        panic!("missing file should fail");
    };

    assert_eq!(err.kind, AssetError::LoadError.kind);
    assert_eq!(err.asset_type, Some(Animation::asset_name()));
    assert_eq!(err.path.as_deref(), Some(path.as_path()));
}

#[test]
fn animation_round_trips_standalone_json_file() {
    let context = test_game_context().assets.lock_read();
    let path = std::env::temp_dir().join(format!("animation-{}.cxanimclip", Uuid::new_v4()));
    let mut node_keyframes = HashMap::new();
    node_keyframes.insert(
        "Turret".to_string(),
        AnimationKeyFrames {
            positions: vec![VectorKeyFrame {
                value: Vec3::new(1.0, 2.0, 3.0),
                time: 0.25,
            }],
            rotations: vec![QuatKeyFrame {
                value: Unit::<Quat>::from_quaternion(Quat::new(1.0, 0.0, 0.0, 0.0)),
                time: 0.5,
            }],
            scaling: vec![VectorKeyFrame {
                value: Vec3::new(2.0, 2.0, 2.0),
                time: 0.75,
            }],
        },
    );
    let animation = Animation {
        node_keyframes,
        duration: 2.0,
        ticks_per_second: 24.0,
    };

    animation.to_file(&path).expect("failed to write animation");
    let loaded = Animation::from_file(&context, &path)
        .expect("failed to read animation")
        .asset;
    let _ = fs::remove_file(&path);

    assert_eq!(loaded.duration, 2.0);
    assert_eq!(loaded.ticks_per_second, 24.0);
    let keyframes = loaded
        .node_keyframes
        .get("Turret")
        .expect("missing node keyframes");
    assert_eq!(keyframes.positions[0].value, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(keyframes.positions[0].time, 0.25);
    assert_eq!(keyframes.rotations[0].value.quaternion().w, 1.0);
    assert_eq!(keyframes.rotations[0].time, 0.5);
    assert_eq!(keyframes.scaling[0].value, Vec3::new(2.0, 2.0, 2.0));
    assert_eq!(keyframes.scaling[0].time, 0.75);
}
