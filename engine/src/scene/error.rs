use std::fmt;

/// Errors produced by scene-specific operations.
#[derive(Debug)]
pub enum SceneError {
    /// The requested node did not exist in the scene graph.
    InvalidNodeId,
    /// The requested component was not attached to the target object.
    ComponentNotBound,
    /// Prefab creation could not be completed.
    UnableToCreatePrefab,
    /// Prefab data was malformed and could not be instantiated.
    InvalidPrefab(String),
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SceneError::InvalidNodeId => write!(f, "invalid node ID"),
            SceneError::ComponentNotBound => write!(f, "component not bound to entity specified"),
            SceneError::UnableToCreatePrefab => write!(f, "unable to create prefab"),
            SceneError::InvalidPrefab(reason) => write!(f, "invalid prefab: {reason}"),
        }
    }
}

impl std::error::Error for SceneError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            SceneError::InvalidNodeId
            | SceneError::ComponentNotBound
            | SceneError::UnableToCreatePrefab
            | SceneError::InvalidPrefab(_) => None,
        }
    }
}
