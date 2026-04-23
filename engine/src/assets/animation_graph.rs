use crate as engine;
use crate::assets::animation::Animation;
use crate::assets::error::AssetError;
use crate::assets::{Asset, AssetRef, AssetRegistry, LoadedAsset};
use crate::context::ReadOnlyAssetContext;
use crate::math::Distance;
use eframe::emath::Pos2;
use engine_derive::TypeUuid;
use lerp::Lerp;
use petgraph::prelude::StableGraph;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Error;
use std::ops::{Deref, DerefMut};
use std::path::Path;
use uuid::Uuid;

#[derive(Default, Clone, Serialize, Deserialize)]
#[repr(C)]
pub struct BlendTreeMotion<T: Default + Clone> {
    pub threshold: T,
    pub motion: AnimationMotion,
}

#[derive(Clone, Serialize, Deserialize)]
#[repr(C)]
pub enum AnimationMotion {
    AnimationClip(AnimationClip),
    BlendTree1D(BlendTree<1>),
    BlendTree2D(BlendTree<2>),
}

#[derive(Clone, Serialize, Deserialize)]
#[repr(C)]
pub struct AnimationClip {
    pub speed: f32,
    pub animation: AssetRef<Animation>,
}

impl Default for AnimationClip {
    fn default() -> Self {
        Self {
            speed: 1.0,
            animation: Default::default(),
        }
    }
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[repr(C)]
pub struct BlendTree<const N: usize>
where
    [f32; N]: Default + Serialize + for<'a> Deserialize<'a>,
{
    pub parameters: Vec<Uuid>,
    pub motions: Vec<BlendTreeMotion<[f32; N]>>,
}

impl<const N: usize> BlendTree<N>
where
    [f32; N]: Default + Serialize + for<'a> Deserialize<'a>,
{
    pub const fn dimensions() -> usize
    where
        [f32; N]: Default + Serialize + for<'a> Deserialize<'a>,
    {
        N
    }
}

impl<const N: usize> BlendTree<N>
where
    [f32; N]: Default + Serialize + for<'a> Deserialize<'a>,
{
    pub fn nearest_neighbors(
        &self,
        n: usize,
        parameters: &HashMap<Uuid, AnimationParameterValue>,
    ) -> Vec<(f32, &BlendTreeMotion<[f32; N]>)>
    where
        [f32; N]: Default + Serialize + for<'a> Deserialize<'a>,
    {
        let value: [f32; N] = std::array::from_fn(|idx| {
            self.parameters
                .get(idx)
                .and_then(|pid| match parameters.get(pid) {
                    Some(AnimationParameterValue::Float(value)) => Some(*value),
                    _ => None,
                })
                .unwrap_or(f32::NAN)
        });
        if value.iter().any(|value| value.is_nan()) {
            return vec![];
        }
        let mut motions = self
            .motions
            .iter()
            .map(|motion| {
                let dist = motion.threshold.distance(&value);
                let weight = if dist < f32::EPSILON {
                    f32::INFINITY
                } else {
                    1.0 / dist.powf(2.0)
                };
                (weight, motion)
            })
            .collect::<Vec<_>>();
        motions.sort_by(|a, b| b.0.total_cmp(&a.0));
        motions.truncate(n);
        if motions[0].0 == f32::INFINITY {
            motions[0].0 = 1.0;
            motions.truncate(1);
        } else {
            let total_weight = motions.iter().map(|(d, _)| *d).sum::<f32>();
            for motion in &mut motions {
                motion.0 /= total_weight;
            }
        }
        motions
    }
}

impl Default for AnimationMotion {
    fn default() -> Self {
        Self::AnimationClip(Default::default())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AnimationNode {
    pub id: Uuid,
    pub name: String,
    pub motion: AnimationMotion,
    pub position: Pos2,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AnimationParameterValue {
    Float(f32),
    Int(i32),
    Bool(bool),
    Trigger,
}

impl AnimationParameterValue {
    pub fn is_float(&self) -> bool {
        matches!(self, Self::Float(_))
    }

    pub fn is_int(&self) -> bool {
        matches!(self, Self::Int(_))
    }

    pub fn is_bool(&self) -> bool {
        matches!(self, Self::Bool(_))
    }

    pub fn is_trigger(&self) -> bool {
        matches!(self, Self::Trigger)
    }
}

impl Lerp<f32> for AnimationParameterValue {
    fn lerp(self, other: Self, t: f32) -> Self {
        match (self, other) {
            (AnimationParameterValue::Float(current), AnimationParameterValue::Float(other)) => {
                AnimationParameterValue::Float(current.lerp(other, t))
            }
            (AnimationParameterValue::Int(current), AnimationParameterValue::Int(other)) => {
                AnimationParameterValue::Int((current as f32).lerp(other as f32, t) as i32)
            }
            (AnimationParameterValue::Bool(current), AnimationParameterValue::Bool(other)) => {
                AnimationParameterValue::Bool(if t >= 0.5 { other } else { current })
            }
            (_, other) => other,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AnimationTransition {
    pub id: Uuid,
    pub name: String,
    pub has_exit_time: bool,
    pub exit_time: f32,
    pub duration: f32,
    pub conditions: Vec<AnimationParameterCondition>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct AnimationParameterCondition {
    pub parameter: Uuid,
    pub condition: AnimationCondition,
}

#[derive(Default, Clone, Copy, Serialize, Deserialize)]
pub enum AnimationCondition {
    #[default]
    None,
    Float(FloatCondition),
    Int(IntCondition),
    Bool(BoolCondition),
    Trigger,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum FloatCondition {
    Less(f32),
    Greater(f32),
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum IntCondition {
    Less(i32),
    Greater(i32),
    Equal(i32),
    NotEqual(i32),
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum BoolCondition {
    True,
    False,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AnimationParameter {
    pub id: Uuid,
    pub name: String,
    pub value: AnimationParameterValue,
}

impl Default for AnimationParameter {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: String::from("Untitled Parameter"),
            value: AnimationParameterValue::Float(0.0),
        }
    }
}

#[derive(TypeUuid, Default, Clone, Serialize, Deserialize)]
#[uuid = "5796ef05-4a2c-4cbf-b70a-4e6e1f2c418a"]
pub struct AnimationGraph {
    pub graph: StableGraph<AnimationNode, AnimationTransition>,
    pub parameters: Vec<AnimationParameter>,
    pub start_node: Option<Uuid>,
}

impl Deref for AnimationGraph {
    type Target = StableGraph<AnimationNode, AnimationTransition>;

    fn deref(&self) -> &Self::Target {
        &self.graph
    }
}

impl DerefMut for AnimationGraph {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.graph
    }
}

impl Asset for AnimationGraph {
    fn asset_name() -> &'static str
    where
        Self: Sized,
    {
        "AnimationGraph"
    }

    fn file_extensions() -> &'static [&'static str]
    where
        Self: Sized,
    {
        &["cxanim"]
    }

    fn from_file(
        _assets: &ReadOnlyAssetContext,
        path: &Path,
    ) -> Result<LoadedAsset<Self>, AssetError>
    where
        Self: Sized,
    {
        LoadedAsset::<Self>::from_json_file(path)
    }

    fn to_file(&self, path: &Path) -> Result<(), Error> {
        AssetRegistry::write_to_file(self, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lerp::Lerp;
    use uuid::Uuid;

    #[test]
    fn lerp_float() {
        let a = AnimationParameterValue::Float(0.0);
        let b = AnimationParameterValue::Float(10.0);
        let AnimationParameterValue::Float(v) = a.lerp(b, 0.5) else {
            panic!()
        };
        assert!((v - 5.0).abs() < 1e-5);
    }

    #[test]
    fn lerp_int() {
        let a = AnimationParameterValue::Int(0);
        let b = AnimationParameterValue::Int(10);
        let AnimationParameterValue::Int(v) = a.lerp(b, 0.8) else {
            panic!()
        };
        assert_eq!(v, 8);
    }

    #[test]
    fn lerp_bool_threshold() {
        let f = AnimationParameterValue::Bool(false);
        let t = AnimationParameterValue::Bool(true);
        let AnimationParameterValue::Bool(v) = f.clone().lerp(t.clone(), 0.4) else {
            panic!()
        };
        assert!(!v);
        let AnimationParameterValue::Bool(v) = f.lerp(t, 0.6) else {
            panic!()
        };
        assert!(v);
    }

    fn make_params(id: Uuid, val: f32) -> HashMap<Uuid, AnimationParameterValue> {
        let mut m = HashMap::new();
        m.insert(id, AnimationParameterValue::Float(val));
        m
    }

    #[test]
    fn nearest_neighbor_exact_match_weight_one() {
        let pid = Uuid::new_v4();
        let tree: BlendTree<1> = BlendTree {
            parameters: vec![pid],
            motions: vec![
                BlendTreeMotion {
                    threshold: [0.0],
                    motion: AnimationMotion::default(),
                },
                BlendTreeMotion {
                    threshold: [1.0],
                    motion: AnimationMotion::default(),
                },
            ],
        };
        let params = make_params(pid, 0.0);
        let neighbors = tree.nearest_neighbors(2, &params);
        assert_eq!(neighbors.len(), 1);
        assert!((neighbors[0].0 - 1.0).abs() < 1e-5);
    }

    #[test]
    fn nearest_neighbors_weights_sum_to_one() {
        let pid = Uuid::new_v4();
        let tree: BlendTree<1> = BlendTree {
            parameters: vec![pid],
            motions: vec![
                BlendTreeMotion {
                    threshold: [0.0],
                    motion: AnimationMotion::default(),
                },
                BlendTreeMotion {
                    threshold: [1.0],
                    motion: AnimationMotion::default(),
                },
            ],
        };
        let params = make_params(pid, 0.5);
        let neighbors = tree.nearest_neighbors(2, &params);
        let total: f32 = neighbors.iter().map(|(w, _)| w).sum();
        assert!((total - 1.0).abs() < 1e-5);
    }

    #[test]
    fn nearest_neighbors_missing_param_returns_empty() {
        let tree: BlendTree<1> = BlendTree {
            parameters: vec![Uuid::new_v4()],
            motions: vec![BlendTreeMotion {
                threshold: [0.0],
                motion: AnimationMotion::default(),
            }],
        };
        let neighbors = tree.nearest_neighbors(2, &HashMap::new());
        assert!(neighbors.is_empty());
    }
}
