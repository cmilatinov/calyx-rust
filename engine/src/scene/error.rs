use std::fmt;

#[derive(Debug)]
pub enum SceneError {
    InvalidNodeId,
    ComponentNotBound
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            SceneError::InvalidNodeId => write!(f, "invalid node ID"),
            SceneError::ComponentNotBound => write!(f, "component not bound to entity specified"),
        }
    }
}

impl std::error::Error for SceneError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            SceneError::InvalidNodeId |
            SceneError::ComponentNotBound => None
        }
    }
}