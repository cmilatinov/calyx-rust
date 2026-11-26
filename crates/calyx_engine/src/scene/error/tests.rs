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
