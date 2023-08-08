use std::fmt;

#[derive(Debug)]
pub enum SceneError {
    InvalidNodeId,
    ComponentNotBound,
    ParseSpecsError(specs::error::Error)
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            SceneError::InvalidNodeId => write!(f, "invalid node ID"),
            SceneError::ComponentNotBound => write!(f, "component not bound to entity specified"),
            SceneError::ParseSpecsError(..) => write!(f, "specs error")
        }
    }
}

impl std::error::Error for SceneError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            SceneError::InvalidNodeId |
            SceneError::ComponentNotBound => None,
            SceneError::ParseSpecsError(ref e) => Some(e)
        }
    }
}

impl From<specs::error::Error> for SceneError {
    fn from(error: specs::error::Error) -> Self {
        SceneError::ParseSpecsError(error)
    }
}