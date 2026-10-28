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
mod tests {
    use super::AssetShaderLoader;
    use crate::test_support::test_registries_with_assets;
    use calyx_shader::ShaderError;
    use std::fs;
    use uuid::Uuid;

    /// The loader resolves includes from any asset root and reports the
    /// canonical paths the registry keys dependencies by.
    #[test]
    fn includes_resolve_from_the_asset_roots() {
        let root = std::env::temp_dir().join(format!("calyx-shader-loader-{}", Uuid::new_v4()));
        let first = root.join("first");
        let second = root.join("second");
        fs::create_dir_all(first.join("shaders")).expect("the first root is created");
        fs::create_dir_all(&second).expect("the second root is created");
        fs::write(second.join("root.wgsl"), "//#include \"nested.wgsl\"\nroot")
            .expect("the root include is written");
        fs::write(first.join("nested.wgsl"), "nested").expect("the nested include is written");
        fs::write(
            first.join("shaders/main.wgsl"),
            "//#include \"root.wgsl\"\nmain",
        )
        .expect("the shader is written");
        fs::write(first.join("broken.wgsl"), "//#include \"missing.wgsl\"")
            .expect("the broken shader is written");

        let registries = test_registries_with_assets(vec![first.clone(), second.clone()]);
        let registry = registries.assets.read();
        let loader = AssetShaderLoader::new(&registry);
        let main = first.join("shaders/main.wgsl");

        let shader = loader.preprocess(&main).expect("the shader preprocesses");
        assert_eq!(shader.source, "nested\nroot\nmain");
        let nested = shader.source_map.location(1).expect("line 1 is mapped");
        assert!(nested.file.ends_with("nested.wgsl"), "{nested:?}");

        let dependencies = loader.dependencies(&main).expect("dependencies collect");
        assert_eq!(dependencies.len(), 2);
        assert!(dependencies.contains(&fs::canonicalize(second.join("root.wgsl")).unwrap()));
        assert!(dependencies.contains(&fs::canonicalize(first.join("nested.wgsl")).unwrap()));

        let error = loader
            .preprocess(&first.join("broken.wgsl"))
            .expect_err("a missing include fails");
        assert!(
            matches!(&error, ShaderError::IncludeNotFound { include, line: 1, .. } if include == "missing.wgsl"),
            "{error}"
        );
        drop(registry);
        fs::remove_dir_all(root).expect("the temp asset roots are removed");
    }
}
