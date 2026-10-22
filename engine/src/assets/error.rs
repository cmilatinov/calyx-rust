use std::fmt::{Display, Write};
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
///
/// The message reads `<kind>[ for <asset_type>][ at <path>][: <source_message>]`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}", describe(self))]
pub struct AssetError {
    /// Broad error category.
    pub kind: AssetErrorKind,
    /// Asset path involved in the failure, when known.
    pub path: Option<PathBuf>,
    /// Asset type involved in the failure, when known.
    pub asset_type: Option<&'static str>,
    /// Displayable source error string, when available.
    ///
    /// It is part of the message rather than an error chain, so `source()`
    /// returns `None`. The field is not named `source` because `thiserror`
    /// treats a field of that name as the wrapped error type.
    pub source_message: Option<String>,
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
            source_message: None,
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
        self.source_message = Some(source.to_string());
        self
    }
}

/// Builds an [`AssetError`] message from its kind and whichever context it has.
fn describe(error: &AssetError) -> String {
    let mut message = String::from(match error.kind {
        AssetErrorKind::NotFound => "asset not found",
        AssetErrorKind::LoadError => "asset load failed",
        AssetErrorKind::AlreadyExists => "asset already exists",
        AssetErrorKind::TypeMismatch => "asset type mismatch",
    });
    if let Some(asset_type) = error.asset_type {
        let _ = write!(message, " for {asset_type}");
    }
    if let Some(path) = &error.path {
        let _ = write!(message, " at {}", path.display());
    }
    if let Some(source) = &error.source_message {
        let _ = write!(message, ": {source}");
    }
    message
}

#[cfg(test)]
mod tests {
    use super::{AssetError, AssetErrorKind};
    use std::error::Error;

    const KINDS: [(AssetErrorKind, &str); 4] = [
        (AssetErrorKind::NotFound, "asset not found"),
        (AssetErrorKind::LoadError, "asset load failed"),
        (AssetErrorKind::AlreadyExists, "asset already exists"),
        (AssetErrorKind::TypeMismatch, "asset type mismatch"),
    ];

    #[test]
    fn formats_every_kind_with_every_combination_of_context() {
        for (kind, label) in KINDS {
            for with_type in [false, true] {
                for with_path in [false, true] {
                    for with_source in [false, true] {
                        let mut error = AssetError::new(kind);
                        let mut expected = label.to_string();
                        if with_type {
                            error = error.with_type("Texture");
                            expected.push_str(" for Texture");
                        }
                        if with_path {
                            error = error.with_path("textures/brick.png");
                            expected.push_str(" at textures/brick.png");
                        }
                        if with_source {
                            error = error.with_source("unexpected end of file");
                            expected.push_str(": unexpected end of file");
                        }
                        assert_eq!(error.to_string(), expected);
                    }
                }
            }
        }
    }

    #[test]
    fn prebuilt_constants_format_as_their_kind() {
        assert_eq!(AssetError::NotFound.to_string(), "asset not found");
        assert_eq!(AssetError::LoadError.to_string(), "asset load failed");
        assert_eq!(
            AssetError::AlreadyExists.to_string(),
            "asset already exists"
        );
        assert_eq!(AssetError::TypeMismatch.to_string(), "asset type mismatch");
    }

    #[test]
    fn string_source_is_part_of_the_message_not_an_error_chain() {
        let error = AssetError::LoadError.with_source("bad header");
        assert!(error.source().is_none());
    }
}
