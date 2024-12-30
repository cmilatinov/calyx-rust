use crate::animator::graph::AnimationParameterValue;
use std::collections::HashMap;

pub struct AnimatorContext {
    parameters: HashMap<String, AnimationParameterValue>,
}
