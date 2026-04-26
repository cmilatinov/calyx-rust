use russimp_ng::RussimpError;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetErrorKind {
    NotFound,
    LoadError,
    AlreadyExists,
    TypeMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetError {
    pub kind: AssetErrorKind,
    pub path: Option<PathBuf>,
    pub asset_type: Option<&'static str>,
    pub source: Option<String>,
}

#[allow(non_upper_case_globals)]
impl AssetError {
    pub const NotFound: Self = Self::new(AssetErrorKind::NotFound);
    pub const LoadError: Self = Self::new(AssetErrorKind::LoadError);
    pub const AlreadyExists: Self = Self::new(AssetErrorKind::AlreadyExists);
    pub const TypeMismatch: Self = Self::new(AssetErrorKind::TypeMismatch);

    pub const fn new(kind: AssetErrorKind) -> Self {
        Self {
            kind,
            path: None,
            asset_type: None,
            source: None,
        }
    }

    pub fn with_path(mut self, path: impl AsRef<Path>) -> Self {
        self.path = Some(path.as_ref().to_path_buf());
        self
    }

    pub fn with_type(mut self, asset_type: &'static str) -> Self {
        self.asset_type = Some(asset_type);
        self
    }

    pub fn with_source(mut self, source: impl Display) -> Self {
        self.source = Some(source.to_string());
        self
    }
}

impl Display for AssetError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let kind = match self.kind {
            AssetErrorKind::NotFound => "asset not found",
            AssetErrorKind::LoadError => "asset load failed",
            AssetErrorKind::AlreadyExists => "asset already exists",
            AssetErrorKind::TypeMismatch => "asset type mismatch",
        };

        write!(f, "{kind}")?;
        if let Some(asset_type) = self.asset_type {
            write!(f, " for {asset_type}")?;
        }
        if let Some(path) = &self.path {
            write!(f, " at {}", path.display())?;
        }
        if let Some(source) = &self.source {
            write!(f, ": {source}")?;
        }
        Ok(())
    }
}

impl Error for AssetError {}

impl From<RussimpError> for AssetError {
    fn from(error: RussimpError) -> Self {
        AssetError::LoadError.with_source(error)
    }
}
