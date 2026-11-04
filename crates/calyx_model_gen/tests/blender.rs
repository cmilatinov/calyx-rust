//! The Blender step, run for real on the sandbox's own models. Ignored by
//! default because it needs Blender; run it with
//! `cargo test -p calyx_model_gen --test blender -- --ignored`.

use std::path::{Path, PathBuf};

use calyx_model_gen::layout::Layout;
use calyx_model_gen::{blender, generate, params};

const BLENDER: &str = r"C:\Program Files\Blender Foundation\Blender 4.0\blender.exe";

#[test]
#[ignore = "needs Blender 4.0 at its default install path"]
fn blender_builds_every_sandbox_model_and_each_passes_the_checks() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let models = params::load_dir(&repo.join("sandbox/asset_sources/models")).unwrap();
    let root: PathBuf = Path::new(env!("CARGO_TARGET_TMPDIR")).join("calyx_model_gen_blender");
    let _ = std::fs::remove_dir_all(&root);
    let layout = Layout::new(&root.join("assets").join("generated")).unwrap();
    let report = generate(&layout, &models, |jobs| {
        blender::build(Path::new(BLENDER), &root.join("work"), jobs)
    })
    .unwrap();
    assert_eq!(report.models, models.len());
}

#[test]
#[ignore = "needs Blender 4.0 at its default install path"]
fn a_mirrored_source_part_keeps_its_faces_out() {
    // A unit cube under a node scaled -1 on x: baking that transform turns
    // the cube inside out unless its winding is reversed with it.
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sources/mirrored_cube.gltf");
    let text = format!(
        "kind = \"import\"\nrole = \"crates\"\nshading = \"flat\"\nsource = {:?}\n\
         credit = {{ title = \"t\", author = \"a\", licence = \"CC0-1.0\", url = \"https://example.com\" }}\n",
        source.to_string_lossy()
    );
    let model = params::Model::parse("mirrored", &text, Path::new("")).unwrap();
    let root: PathBuf = Path::new(env!("CARGO_TARGET_TMPDIR")).join("calyx_model_gen_mirrored");
    let _ = std::fs::remove_dir_all(&root);
    let layout = Layout::new(&root.join("assets").join("generated")).unwrap();
    generate(&layout, &[model], |jobs| {
        blender::build(Path::new(BLENDER), &root.join("work"), jobs)
    })
    .unwrap();
}
