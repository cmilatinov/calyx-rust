//! Checking that `THIRD_PARTY_ASSETS.md` credits every sourced model.

use crate::params::{Model, Shape};

/// Checks that `credits`, the text of `THIRD_PARTY_ASSETS.md`, names each
/// imported model's source file, URL and author, so no sourced model is
/// committed without its licence on record.
pub fn check(credits: &str, models: &[Model]) -> Result<(), String> {
    let mut missing = Vec::new();
    for model in models {
        let Shape::Import { source, credit, .. } = &model.shape else {
            continue;
        };
        let file = source
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        for (what, text) in [
            ("source file", file.as_str()),
            ("URL", credit.url.as_str()),
            ("author", credit.author.as_str()),
        ] {
            if !credits.contains(text) {
                missing.push(format!("{}: its {what} {text:?}", model.name));
            }
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "THIRD_PARTY_ASSETS.md does not credit every sourced model; it lacks\n  {}",
            missing.join("\n  ")
        ))
    }
}

#[cfg(test)]
mod tests;
