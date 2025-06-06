use std::collections::HashMap;
use std::path::Path;

use crate as engine;
use crate::context::ReadOnlyAssetContext;
use crate::utils::TypeUuid;
use nalgebra::Unit;
use nalgebra_glm::{Quat, Vec3};

use super::error::AssetError;
use super::{Asset, LoadedAsset};

#[derive(Debug)]
pub struct VectorKeyFrame {
    pub value: Vec3,
    pub time: f64,
}

#[derive(Debug)]
pub struct QuatKeyFrame {
    pub value: Unit<Quat>,
    pub time: f64,
}

#[derive(Debug)]
pub struct AnimationKeyFrames {
    pub positions: Vec<VectorKeyFrame>,
    pub rotations: Vec<QuatKeyFrame>,
    pub scaling: Vec<VectorKeyFrame>,
}

#[derive(Default, TypeUuid)]
#[uuid = "627dee5d-c2d6-4e3e-9b9e-80e3e601848d"]
pub struct Animation {
    pub node_keyframes: HashMap<String, AnimationKeyFrames>,
    pub duration: f64,
    pub ticks_per_second: f64,
}

impl Asset for Animation {
    fn asset_name() -> &'static str
    where
        Self: Sized,
    {
        "Animation"
    }

    fn file_extensions() -> &'static [&'static str] {
        &[]
    }

    fn from_file(
        _assets: &ReadOnlyAssetContext,
        _path: &Path,
    ) -> Result<LoadedAsset<Self>, AssetError> {
        todo!()
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
                            .map(|p| VectorKeyFrame {
                                value: Vec3::new(p.value.x, p.value.y, p.value.z),
                                time: p.time,
                            })
                            .collect(),
                        rotations: channel
                            .rotation_keys
                            .iter()
                            .map(|r| QuatKeyFrame {
                                value: Unit::<Quat>::from_quaternion(Quat::new(
                                    r.value.w, r.value.x, r.value.y, r.value.z,
                                )),
                                time: r.time,
                            })
                            .collect(),
                        scaling: channel
                            .scaling_keys
                            .iter()
                            .map(|s| VectorKeyFrame {
                                value: Vec3::new(s.value.x, s.value.y, s.value.z),
                                time: s.time,
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
