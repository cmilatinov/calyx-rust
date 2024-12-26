use engine::assets::animation::Animation;
use engine::core::Ref;
use engine::petgraph::prelude::StableGraph;
use engine::uuid::Uuid;

pub enum AnimationNodeType {
    Start,
    Node,
    End,
}

pub struct AnimationNode {
    id: Uuid,
    ty: AnimationNodeType,
    name: String,
    animation: Option<Ref<Animation>>,
}

pub struct AnimationParameter {
    name: String,
    value: AnimationParameterValue,
}

pub enum AnimationParameterValue {
    Float(f32),
    Int(i32),
    Bool(bool),
    Trigger,
}

pub struct AnimationTransition {
    id: Uuid,
    conditions: Vec<AnimationParameterCondition>,
}

pub struct AnimationParameterCondition {
    parameter: String,
    condition: AnimationCondition,
}

pub enum AnimationCondition {
    Float(FloatCondition),
    Int(IntCondition),
    Bool(BoolCondition),
    Trigger,
}

pub enum FloatCondition {
    Less(f32),
    Greater(f32),
}

pub enum IntCondition {
    Less(i32),
    Greater(i32),
    Equal(i32),
    NotEqual(i32),
}

pub enum BoolCondition {
    True,
    False,
}

pub struct AnimationGraph {
    start_node: Uuid,
    graph: StableGraph<AnimationNode, AnimationTransition>,
}
