use std::collections::HashMap;
use std::path::Path;

use glm::{Quat, Vec3};

use crate as engine;
use crate::utils::TypeUuid;

use super::error::AssetError;
use super::Asset;

struct PositionKeyFrame {
    position: Vec3,
    time: f64,
}

struct RotationKeyFrame {
    rotation: Quat,
    time: f64,
}

struct AnimationKeyFrames {
    positions: Vec<PositionKeyFrame>,
    rotations: Vec<RotationKeyFrame>,
}

#[derive(Default, TypeUuid)]
#[uuid = "627dee5d-c2d6-4e3e-9b9e-80e3e601848d"]
pub struct Animation {
    node_keyframes: HashMap<String, AnimationKeyFrames>,
    duration: f64,
    ticks_per_second: f64,
}

impl Asset for Animation {
    fn get_file_extensions() -> &'static [&'static str] {
        &[]
    }

    fn from_file(_path: &Path) -> Result<super::LoadedAsset<Self>, super::error::AssetError> {
        Err(AssetError::LoadError)
    }
}

impl Animation {
    pub fn from_russimp_animation(animation: &russimp::animation::Animation) -> Self {
        let ticks_per_second = animation.ticks_per_second;
        let duration = animation.duration;
        let node_keyframes = animation
            .channels
            .iter()
            .map(|channel| {
                (
                    channel.name.clone(),
                    AnimationKeyFrames {
                        positions: channel
                            .position_keys
                            .iter()
                            .map(|p| PositionKeyFrame {
                                position: Vec3::new(p.value.x, p.value.y, p.value.z),
                                time: p.time,
                            })
                            .collect(),
                        rotations: channel
                            .rotation_keys
                            .iter()
                            .map(|r| RotationKeyFrame {
                                rotation: Quat::new(r.value.x, r.value.y, r.value.z, r.value.w),
                                time: r.time,
                            })
                            .collect(),
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        Self {
            node_keyframes,
            duration,
            ticks_per_second,
        }
    }
}
