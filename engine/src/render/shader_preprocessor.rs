use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use regex::Regex;

use crate::assets::AssetRegistry;

pub struct ShaderPreprocessor;

impl ShaderPreprocessor {
    pub fn load_shader_source(path: &Path) -> std::io::Result<String> {
        let regex = Regex::new(r#"(?m)^\s*//#include\s+"([^"]*)"\s*$"#).unwrap();
        let source: Cow<str> = Cow::Owned(std::fs::read_to_string(path)?);
        let result = regex.replace_all(source.as_ref(), |c: &regex::Captures| {
            if let Some(path) = c.get(1).map(|m| PathBuf::from(m.as_str())) {
                for asset_path in AssetRegistry::get().asset_paths().iter() {
                    let full_path = asset_path.join(&path);
                    if let Ok(src) = Self::load_shader_source(&full_path) {
                        return Cow::Owned(src);
                    }
                }
            }
            return Cow::Borrowed("");
        });
        Ok(result.into_owned())
    }
}
