use russimp::RussimpError;

#[derive(Debug)]
pub enum AssetError {
    NotFound,
    LoadError,
    AssetAlreadyExists,
    TypeMismatch,
}

impl From<RussimpError> for AssetError {
    fn from(_error: RussimpError) -> Self {
        AssetError::LoadError
    }
}
