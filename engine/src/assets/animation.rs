use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Error};
use std::path::Path;

use crate as engine;
use crate::context::ReadOnlyAssetContext;
use crate::utils::TypeUuid;
use nalgebra::Unit;
use nalgebra_glm::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use super::error::AssetError;
use super::{Asset, LoadedAsset};

/// Position or scale keyframe sampled from an animation channel.
#[derive(Debug)]
pub struct VectorKeyFrame {
    /// Sampled vector value.
    pub value: Vec3,
    /// Keyframe time in animation ticks.
    pub time: f64,
}

/// Rotation keyframe sampled from an animation channel.
#[derive(Debug)]
pub struct QuatKeyFrame {
    /// Sampled quaternion value.
    pub value: Unit<Quat>,
    /// Keyframe time in animation ticks.
    pub time: f64,
}

/// Per-node animation channel data.
#[derive(Debug)]
pub struct AnimationKeyFrames {
    /// Translation keys.
    pub positions: Vec<VectorKeyFrame>,
    /// Rotation keys.
    pub rotations: Vec<QuatKeyFrame>,
    /// Scale keys.
    pub scaling: Vec<VectorKeyFrame>,
}

/// Standalone skeletal animation clip asset.
#[derive(Default, TypeUuid)]
#[uuid = "627dee5d-c2d6-4e3e-9b9e-80e3e601848d"]
pub struct Animation {
    /// Animation channels keyed by node name.
    pub node_keyframes: HashMap<String, AnimationKeyFrames>,
    /// Clip duration in animation ticks.
    pub duration: f64,
    /// Tick frequency used by the source animation.
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
        &["cxanimclip"]
    }

    fn from_file(
        _assets: &ReadOnlyAssetContext,
        path: &Path,
    ) -> Result<LoadedAsset<Self>, AssetError> {
        let file = File::open(path).map_err(|err| {
            AssetError::LoadError
                .with_path(path)
                .with_type(Self::asset_name())
                .with_source(err)
        })?;
        let reader = BufReader::new(file);
        let data: AnimationData = serde_json::from_reader(reader).map_err(|err| {
            AssetError::LoadError
                .with_path(path)
                .with_type(Self::asset_name())
                .with_source(err)
        })?;
        Ok(LoadedAsset::new(data.into()))
    }

    fn to_file(&self, path: &Path) -> Result<(), Error> {
        let file = File::create(path)?;
        serde_json::to_writer_pretty(file, &AnimationData::from(self)).map_err(Error::from)
    }
}

impl Animation {
    /// Converts a Russimp animation into the engine's standalone clip format.
    pub fn from_russimp_animation(animation: &russimp_ng::animation::Animation) -> Self {
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

#[derive(Serialize, Deserialize)]
struct AnimationData {
    node_keyframes: HashMap<String, AnimationKeyFramesData>,
    duration: f64,
    ticks_per_second: f64,
}

#[derive(Serialize, Deserialize)]
struct AnimationKeyFramesData {
    positions: Vec<VectorKeyFrameData>,
    rotations: Vec<QuatKeyFrameData>,
    scaling: Vec<VectorKeyFrameData>,
}

#[derive(Serialize, Deserialize)]
struct VectorKeyFrameData {
    value: [f32; 3],
    time: f64,
}

#[derive(Serialize, Deserialize)]
struct QuatKeyFrameData {
    value: [f32; 4],
    time: f64,
}

impl From<AnimationData> for Animation {
    fn from(value: AnimationData) -> Self {
        Self {
            node_keyframes: value
                .node_keyframes
                .into_iter()
                .map(|(node, keyframes)| (node, keyframes.into()))
                .collect(),
            duration: value.duration,
            ticks_per_second: value.ticks_per_second,
        }
    }
}

impl From<AnimationKeyFramesData> for AnimationKeyFrames {
    fn from(value: AnimationKeyFramesData) -> Self {
        Self {
            positions: value.positions.into_iter().map(Into::into).collect(),
            rotations: value.rotations.into_iter().map(Into::into).collect(),
            scaling: value.scaling.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<VectorKeyFrameData> for VectorKeyFrame {
    fn from(value: VectorKeyFrameData) -> Self {
        Self {
            value: Vec3::new(value.value[0], value.value[1], value.value[2]),
            time: value.time,
        }
    }
}

impl From<QuatKeyFrameData> for QuatKeyFrame {
    fn from(value: QuatKeyFrameData) -> Self {
        Self {
            value: Unit::<Quat>::from_quaternion(Quat::new(
                value.value[0],
                value.value[1],
                value.value[2],
                value.value[3],
            )),
            time: value.time,
        }
    }
}

impl From<&Animation> for AnimationData {
    fn from(value: &Animation) -> Self {
        Self {
            node_keyframes: value
                .node_keyframes
                .iter()
                .map(|(node, keyframes)| (node.clone(), keyframes.into()))
                .collect(),
            duration: value.duration,
            ticks_per_second: value.ticks_per_second,
        }
    }
}

impl From<&AnimationKeyFrames> for AnimationKeyFramesData {
    fn from(value: &AnimationKeyFrames) -> Self {
        Self {
            positions: value.positions.iter().map(Into::into).collect(),
            rotations: value.rotations.iter().map(Into::into).collect(),
            scaling: value.scaling.iter().map(Into::into).collect(),
        }
    }
}

impl From<&VectorKeyFrame> for VectorKeyFrameData {
    fn from(value: &VectorKeyFrame) -> Self {
        Self {
            value: value.value.into(),
            time: value.time,
        }
    }
}

impl From<&QuatKeyFrame> for QuatKeyFrameData {
    fn from(value: &QuatKeyFrame) -> Self {
        let quat = value.value.quaternion();
        Self {
            value: [quat.w, quat.i, quat.j, quat.k],
            time: value.time,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Animation, AnimationKeyFrames, QuatKeyFrame, VectorKeyFrame};
    use crate::assets::error::AssetError;
    use crate::assets::Asset;
    use crate::test_utils::test_game_context;
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
}
