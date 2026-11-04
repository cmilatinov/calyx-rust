//! The sandbox's own parameter files parse, and every sourced model among
//! them is credited in `sandbox/THIRD_PARTY_ASSETS.md`.

use std::path::Path;

use calyx_model_gen::{credits, params};

#[test]
fn the_sandbox_models_parse_and_their_sources_are_credited() {
    let sandbox = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sandbox");
    let models = params::load_dir(&sandbox.join("asset_sources/models")).unwrap();
    let text = std::fs::read_to_string(sandbox.join("THIRD_PARTY_ASSETS.md")).unwrap();
    assert_eq!(credits::check(&text, &models), Ok(()));
    for model in &models {
        if let params::Shape::Import { source, .. } = &model.shape {
            assert!(source.is_file(), "{} is committed", source.display());
        }
    }
}
