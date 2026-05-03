use russimp_ng::RussimpError;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};

/// Broad asset error categories surfaced by loading and registry operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetErrorKind {
    /// Requested asset metadata or file was not found.
    NotFound,
    /// Asset data could not be decoded or initialized.
    LoadError,
    /// Attempted to create an asset that already exists.
    AlreadyExists,
    /// Loaded asset type did not match the expected type.
    TypeMismatch,
}

/// Structured asset error with optional path, type, and source context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetError {
    /// Broad error category.
    pub kind: AssetErrorKind,
    /// Asset path involved in the failure, when known.
    pub path: Option<PathBuf>,
    /// Asset type involved in the failure, when known.
    pub asset_type: Option<&'static str>,
    /// Displayable source error string, when available.
    pub source: Option<String>,
}

#[allow(non_upper_case_globals)]
impl AssetError {
    /// Prebuilt `NotFound` error value.
    pub const NotFound: Self = Self::new(AssetErrorKind::NotFound);
    /// Prebuilt `LoadError` error value.
    pub const LoadError: Self = Self::new(AssetErrorKind::LoadError);
    /// Prebuilt `AlreadyExists` error value.
    pub const AlreadyExists: Self = Self::new(AssetErrorKind::AlreadyExists);
    /// Prebuilt `TypeMismatch` error value.
    pub const TypeMismatch: Self = Self::new(AssetErrorKind::TypeMismatch);

    /// Creates a new asset error of `kind`.
    pub const fn new(kind: AssetErrorKind) -> Self {
        Self {
            kind,
            path: None,
            asset_type: None,
            source: None,
        }
    }

    /// Attaches a filesystem path to the error.
    pub fn with_path(mut self, path: impl AsRef<Path>) -> Self {
        self.path = Some(path.as_ref().to_path_buf());
        self
    }

    /// Attaches an asset type label to the error.
    pub fn with_type(mut self, asset_type: &'static str) -> Self {
        self.asset_type = Some(asset_type);
        self
    }

    /// Attaches a source error string.
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
