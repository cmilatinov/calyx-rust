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
mod tests {
    use super::SceneError;
    use std::error::Error;

    #[test]
    fn formats_every_variant() {
        let cases = [
            (SceneError::InvalidNodeId, "invalid node ID"),
            (
                SceneError::ComponentNotBound,
                "component not bound to entity specified",
            ),
            (SceneError::UnableToCreatePrefab, "unable to create prefab"),
            (
                SceneError::InvalidPrefab("root object 3 is missing".to_string()),
                "invalid prefab: root object 3 is missing",
            ),
            (
                SceneError::HierarchyCycle,
                "cannot parent a game object under its own subtree",
            ),
        ];
        for (error, expected) in cases {
            assert_eq!(error.to_string(), expected);
            assert!(error.source().is_none());
        }
    }
}
