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
    use crate::test_support::{test_registries_with_assets, TestAssets};
    use calyx_shader::ShaderError;
    use std::collections::HashSet;
    use std::fs;
    use std::path::{Path, PathBuf};
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

    fn wgsl_files(dir: &Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("the shader dir is readable") {
            let path = entry.expect("the shader dir lists").path();
            if path.is_dir() {
                wgsl_files(&path, files);
            } else if path.extension().is_some_and(|ext| ext == "wgsl") {
                files.push(path);
            }
        }
    }

    /// Every shader under the engine's `assets/shaders/` sits in a purpose
    /// folder and resolves each of its includes through the asset roots, as
    /// the engine loads it. A shader no other file includes also
    /// preprocesses and passes naga's validation, so a moved file or a
    /// stale include path fails here instead of when the engine first
    /// loads the shader. A module, such as `common/curve.wgsl` with its
    /// parameters, is checked through the shaders that include it.
    #[test]
    fn every_engine_shader_resolves_its_includes_and_preprocesses() {
        let assets = TestAssets::engine();
        let shaders_root = assets.engine_root().join("shaders");
        let mut files = Vec::new();
        wgsl_files(&shaders_root, &mut files);
        let files: Vec<PathBuf> = files
            .iter()
            .map(|path| fs::canonicalize(path).expect("the shader exists"))
            .collect();
        let shaders_root = fs::canonicalize(&shaders_root).expect("the shader dir exists");

        let loose: Vec<_> = files
            .iter()
            .filter(|path| path.parent() == Some(shaders_root.as_path()))
            .collect();
        assert!(
            loose.is_empty(),
            "shaders belong in a purpose folder of assets/shaders/: {loose:?}"
        );

        let registries = assets.registries();
        let registry = registries.assets.read();
        let loader = AssetShaderLoader::new(&registry);
        let mut modules = HashSet::new();
        for path in &files {
            let dependencies = loader
                .dependencies(path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            modules.extend(dependencies);
        }
        let shaders: Vec<_> = files
            .iter()
            .filter(|path| !modules.contains(*path))
            .collect();
        for path in &shaders {
            let shader = loader
                .preprocess(path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let module = naga::front::wgsl::parse_str(&shader.source)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|error| panic!("{}: {error:?}", path.display()));
        }

        let in_folder = |name: &str| shaders_root.join(name);
        assert!(
            modules.contains(&in_folder("common/curve.wgsl")),
            "{modules:?}"
        );
        assert!(
            shaders.contains(&&in_folder("mesh/pbr.wgsl")),
            "{shaders:?}"
        );
        assert!(shaders.len() > 20, "only {} shaders", shaders.len());
    }
}
