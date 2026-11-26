/// Errors produced by scene-specific operations.
#[derive(Debug, thiserror::Error)]
pub enum SceneError {
    /// The requested node did not exist in the scene graph.
    #[error("invalid node ID")]
    InvalidNodeId,
    /// The requested component was not attached to the target object.
    #[error("component not bound to entity specified")]
    ComponentNotBound,
    /// Prefab creation could not be completed.
    #[error("unable to create prefab")]
    UnableToCreatePrefab,
    /// Prefab data was malformed and could not be instantiated.
    #[error("invalid prefab: {0}")]
    InvalidPrefab(String),
    /// Reparenting would place an object under itself or one of its descendants.
    #[error("cannot parent a game object under its own subtree")]
    HierarchyCycle,
}

#[cfg(test)]
mod tests;
