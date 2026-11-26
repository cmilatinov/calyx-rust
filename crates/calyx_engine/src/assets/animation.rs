use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Error};
use std::path::Path;

use crate as calyx_engine;
use crate::context::ReadOnlyAssetContext;
use crate::utils::TypeUuid;
use nalgebra::Unit;
use nalgebra_glm::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use super::error::AssetError;
use super::import::ImportedAnimation;
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
    /// Builds a clip from an imported animation, whose key times are in seconds.
    pub(crate) fn from_imported(animation: ImportedAnimation) -> Self {
        Self {
            node_keyframes: animation.channels,
            duration: animation.duration_seconds,
            ticks_per_second: 1.0,
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
mod tests;
