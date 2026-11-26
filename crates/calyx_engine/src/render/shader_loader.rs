use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use calyx_shader::{Preprocessed, ShaderError, ShaderLoader};

use crate::assets::AssetRegistry;

/// Loads shader files for the [`calyx_shader`] preprocessor from the asset
/// registry's roots. An include names a file relative to an asset root,
/// whatever file includes it, and the first root that has it wins.
///
/// Paths are canonical, so a file has one identity however it is reached.
pub struct AssetShaderLoader<'a> {
    registry: &'a AssetRegistry,
}

impl<'a> AssetShaderLoader<'a> {
    pub fn new(registry: &'a AssetRegistry) -> Self {
        Self { registry }
    }

    /// Preprocesses the shader at `path`; see [`calyx_shader::preprocess`].
    pub fn preprocess(&self, path: &Path) -> Result<Preprocessed, ShaderError> {
        calyx_shader::preprocess(self, &canonical(path))
    }

    /// The files the shader at `path` includes, transitively, as
    /// [`std::fs::canonicalize`] gives them, which keys the registry's
    /// shader dependencies.
    pub fn dependencies(&self, path: &Path) -> Result<HashSet<PathBuf>, ShaderError> {
        Ok(calyx_shader::dependencies(self, &canonical(path))?
            .into_iter()
            .map(|path| std::fs::canonicalize(&path).unwrap_or(path))
            .collect())
    }
}

impl ShaderLoader for AssetShaderLoader<'_> {
    fn resolve(&self, _parent: &Path, include: &str) -> Option<PathBuf> {
        self.registry
            .asset_paths()
            .iter()
            .map(|asset_path| asset_path.join(include))
            .find(|path| path.exists())
            .map(|path| canonical(&path))
    }

    fn read(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }
}

/// `path` made canonical without Windows' verbatim prefix, which shader
/// errors would show, or `path` itself when it does not exist.
fn canonical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests;
