use russimp::RussimpError;

#[derive(Debug)]
pub enum AssetError {
    NotFound,
    LoadError,
    AssetAlreadyExists
}

impl From<RussimpError> for AssetError {
    fn from(_error: RussimpError) -> Self {
        AssetError::LoadError
    }
}