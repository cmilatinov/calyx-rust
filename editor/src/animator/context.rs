use engine::uuid::Uuid;
use std::collections::HashMap;

pub struct AnimatorContext {
    parameters: HashMap<Uuid, AnimatorParameter>,
}

pub struct AnimatorParameter {
    id: Uuid,
    name: String,
    value: AnimatorValue,
}

pub enum AnimatorValue {
    Int(i32),
    Float(f32),
    Bool(bool),
    Trigger,
}
