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
mod tests {
    use super::*;
    use std::path::Path;

    fn wall() -> Model {
        Model::parse(
            "wall_low",
            r#"
kind = "import"
role = "barriers"
source = "sources/wall-low.glb"
credit = { title = "Wall (low)", author = "Kenney", licence = "CC0-1.0", url = "https://example.com/kit" }
"#,
            Path::new("models"),
        )
        .unwrap()
    }

    #[test]
    fn a_credited_import_passes() {
        let credits = "## Wall\n- Source: https://example.com/kit\n- Author: Kenney\n- File: `wall-low.glb`\n";
        assert_eq!(check(credits, &[wall()]), Ok(()));
    }

    #[test]
    fn an_uncredited_import_fails_naming_what_is_missing() {
        let err = check(
            "## Wall\n- Author: Kenney\n- File: wall-low.glb\n",
            &[wall()],
        )
        .unwrap_err();
        assert!(err.contains("wall_low: its URL"), "{err}");
        assert!(!err.contains("author"), "{err}");
    }

    #[test]
    fn generated_models_need_no_credit() {
        let rock = Model::parse(
            "rock",
            "kind = \"rock\"\nrole = \"rocks\"\nsize = [1, 1, 1]",
            Path::new(""),
        )
        .unwrap();
        assert_eq!(check("", &[rock]), Ok(()));
    }
}
