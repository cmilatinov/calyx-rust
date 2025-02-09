use crate as engine;
use crate::assets::animation::Animation;
use crate::assets::error::AssetError;
use crate::assets::{Asset, AssetRegistry, LoadedAsset};
use crate::core::Ref;
use eframe::emath::Pos2;
use engine_derive::TypeUuid;
use glm::Vec2;
use petgraph::prelude::StableGraph;
use serde::{Deserialize, Serialize};
use std::io::Error;
use std::ops::{Deref, DerefMut};
use std::path::Path;
use uuid::Uuid;

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct BlendTreeMotion<T: Default + Clone> {
    pub threshold: T,
    pub motion: AnimationMotion,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum AnimationMotion {
    AnimationClip(AnimationClip),
    BlendTree1D(BlendTree1D),
    BlendTree2D(BlendTree2D),
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AnimationClip {
    pub speed: f32,
    pub animation: Option<Ref<Animation>>,
}

impl Default for AnimationClip {
    fn default() -> Self {
        Self {
            speed: 1.0,
            animation: None,
        }
    }
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct BlendTree1D {
    pub parameter: Uuid,
    pub motions: Vec<BlendTreeMotion<f32>>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct BlendTree2D {
    pub parameters: (Uuid, Uuid),
    pub motions: Vec<BlendTreeMotion<Vec2>>,
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

#[derive(Clone, Copy, Serialize, Deserialize)]
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

    fn from_file(path: &Path) -> Result<LoadedAsset<Self>, AssetError>
    where
        Self: Sized,
    {
        LoadedAsset::from_json_file(path)
    }

    fn to_file(&self, path: &Path) -> Result<(), Error> {
        AssetRegistry::write_to_file(self, path)
    }
}

impl AnimationGraph {
    pub fn split(
        &mut self,
    ) -> (
        &mut Vec<AnimationParameter>,
        &mut StableGraph<AnimationNode, AnimationTransition>,
        &mut Option<Uuid>,
    ) {
        (&mut self.parameters, &mut self.graph, &mut self.start_node)
    }
}
