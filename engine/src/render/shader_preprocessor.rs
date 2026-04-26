use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use regex::Regex;

use crate::assets::AssetRegistry;

pub struct ShaderPreprocessor;

impl ShaderPreprocessor {
    pub fn load_shader_source(
        asset_registry: &AssetRegistry,
        path: &Path,
    ) -> std::io::Result<String> {
        let mut include_stack = HashSet::new();
        Self::load_shader_source_inner(asset_registry, path, &mut include_stack)
    }

    fn load_shader_source_inner(
        asset_registry: &AssetRegistry,
        path: &Path,
        include_stack: &mut HashSet<PathBuf>,
    ) -> io::Result<String> {
        let path_key = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if !include_stack.insert(path_key.clone()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("shader include cycle detected at {}", path.display()),
            ));
        }

        let source = std::fs::read_to_string(path)?;
        let mut result = String::with_capacity(source.len());
        let mut last_end = 0;
        for captures in Self::include_regex().captures_iter(&source) {
            let Some(match_range) = captures.get(0) else {
                continue;
            };
            let include_path = captures
                .get(1)
                .map(|m| PathBuf::from(m.as_str()))
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("invalid shader include in {}", path.display()),
                    )
                })?;

            result.push_str(&source[last_end..match_range.start()]);
            result.push_str(&Self::load_include(
                asset_registry,
                path,
                &include_path,
                include_stack,
            )?);
            last_end = match_range.end();
        }
        result.push_str(&source[last_end..]);
        include_stack.remove(&path_key);
        Ok(result)
    }

    fn load_include(
        asset_registry: &AssetRegistry,
        parent_path: &Path,
        include_path: &Path,
        include_stack: &mut HashSet<PathBuf>,
    ) -> io::Result<String> {
        for asset_path in asset_registry.asset_paths().iter() {
            let full_path = asset_path.join(include_path);
            if full_path.exists() {
                return Self::load_shader_source_inner(asset_registry, &full_path, include_stack);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "shader include '{}' referenced by {} was not found",
                include_path.display(),
                parent_path.display()
            ),
        ))
    }

    fn include_regex() -> &'static Regex {
        static INCLUDE_REGEX: OnceLock<Regex> = OnceLock::new();
        INCLUDE_REGEX.get_or_init(|| {
            Regex::new(r#"(?m)^\s*//#include\s+"([^"]*)"\s*$"#)
                .expect("shader include regex must compile")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ShaderPreprocessor;
    use crate::test_utils::test_registries_with_assets;
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn expands_shader_includes() {
        let asset_path =
            std::env::temp_dir().join(format!("calyx-shader-include-{}", Uuid::new_v4()));
        fs::create_dir_all(asset_path.join("shaders")).expect("failed to create shader dir");
        fs::write(asset_path.join("common.wgsl"), "let a = 1u;").expect("failed to write include");
        fs::write(
            asset_path.join("shaders/main.wgsl"),
            "before\n//#include \"common.wgsl\"\nafter",
        )
        .expect("failed to write shader");

        let registries = test_registries_with_assets(vec![asset_path.clone()]);
        let registry = registries.assets.read();
        let source = ShaderPreprocessor::load_shader_source(
            &registry,
            &asset_path.join("shaders/main.wgsl"),
        )
        .expect("shader include should expand");

        assert_eq!(source, "before\nlet a = 1u;\nafter");
        fs::remove_dir_all(asset_path).expect("failed to remove temp shader dir");
    }

    #[test]
    fn reports_missing_shader_includes() {
        let asset_path =
            std::env::temp_dir().join(format!("calyx-missing-include-{}", Uuid::new_v4()));
        fs::create_dir_all(&asset_path).expect("failed to create shader dir");
        fs::write(asset_path.join("main.wgsl"), "//#include \"missing.wgsl\"")
            .expect("failed to write shader");

        let registries = test_registries_with_assets(vec![asset_path.clone()]);
        let registry = registries.assets.read();
        let err = ShaderPreprocessor::load_shader_source(&registry, &asset_path.join("main.wgsl"))
            .expect_err("missing include should error");

        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        fs::remove_dir_all(asset_path).expect("failed to remove temp shader dir");
    }

    #[test]
    fn reports_shader_include_cycles() {
        let asset_path =
            std::env::temp_dir().join(format!("calyx-include-cycle-{}", Uuid::new_v4()));
        fs::create_dir_all(&asset_path).expect("failed to create shader dir");
        fs::write(asset_path.join("a.wgsl"), "//#include \"b.wgsl\"")
            .expect("failed to write shader a");
        fs::write(asset_path.join("b.wgsl"), "//#include \"a.wgsl\"")
            .expect("failed to write shader b");

        let registries = test_registries_with_assets(vec![asset_path.clone()]);
        let registry = registries.assets.read();
        let err = ShaderPreprocessor::load_shader_source(&registry, &asset_path.join("a.wgsl"))
            .expect_err("include cycle should error");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        fs::remove_dir_all(asset_path).expect("failed to remove temp shader dir");
    }
}
