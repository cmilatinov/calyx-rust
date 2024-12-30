use engine::assets::animation::Animation;
use engine::core::Ref;
use engine::glm::Vec2;
use engine::petgraph::prelude::StableGraph;
use engine::uuid::Uuid;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct BlendTreeMotion<T: Clone> {
    threshold: T,
    motion: AnimationMotion,
}

#[derive(Serialize, Deserialize)]
pub enum AnimationMotion {
    AnimationClip {
        threshold: f32,
        speed: f32,
        animation: Option<Ref<Animation>>,
    },
    BlendTree1D {
        parameter: String,
        motions: Vec<BlendTreeMotion<f32>>,
    },
    BlendTree2D {
        parameters: (String, String),
        motions: Vec<BlendTreeMotion<Vec2>>,
    },
}

#[derive(Serialize, Deserialize)]
pub struct AnimationNode {
    id: Uuid,
    name: String,
    motion: AnimationMotion,
}

pub enum AnimationParameterValue {
    Float(f32),
    Int(i32),
    Bool(bool),
    Trigger,
}

#[derive(Serialize, Deserialize)]
pub struct AnimationTransition {
    id: Uuid,
    conditions: Vec<AnimationParameterCondition>,
}

#[derive(Serialize, Deserialize)]
pub struct AnimationParameterCondition {
    parameter: String,
    condition: AnimationCondition,
}

#[derive(Serialize, Deserialize)]
pub enum AnimationCondition {
    Float(FloatCondition),
    Int(IntCondition),
    Bool(BoolCondition),
    Trigger,
}

#[derive(Serialize, Deserialize)]
pub enum FloatCondition {
    Less(f32),
    Greater(f32),
}

#[derive(Serialize, Deserialize)]
pub enum IntCondition {
    Less(i32),
    Greater(i32),
    Equal(i32),
    NotEqual(i32),
}

#[derive(Serialize, Deserialize)]
pub enum BoolCondition {
    True,
    False,
}

#[derive(Serialize, Deserialize)]
pub struct AnimationGraph {
    start_node: Uuid,
    graph: StableGraph<AnimationNode, AnimationTransition>,
}
