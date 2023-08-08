use std::fmt;
use russimp::RussimpError;

#[derive(Debug)]
pub enum MeshError {
    MeshNotFound,
    ParseRussimpError(RussimpError)
}

impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            MeshError::MeshNotFound => write!(f, "no mesh found in the scene"),
            MeshError::ParseRussimpError(..) => write!(f, "russimp error")
        }
    }
}

impl std::error::Error for MeshError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            MeshError::MeshNotFound => None,
            MeshError::ParseRussimpError(ref e) => Some(e)
        }
    }
}

impl From<RussimpError> for MeshError {
    fn from(error: RussimpError) -> Self {
        MeshError::ParseRussimpError(error)
    }
}
