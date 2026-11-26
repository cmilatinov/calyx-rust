use super::*;

fn parse(text: &str) -> Result<Model, String> {
    Model::parse("sample", text, Path::new("models"))
}

#[test]
fn a_box_reads_its_size_bevel_role_and_band() {
    let model = parse(
        r#"
kind = "box"
role = "crates"
outline = 0.07
size = [1.1, 1.1, 1.1]
bevel = 0.12
band = { height = 0.18, overhang = 0.04 }
"#,
    )
    .unwrap();
    assert_eq!(
        model,
        Model {
            name: "sample".into(),
            role: "crates".into(),
            shading: Shading::Smooth { angle: 60.0 },
            outline: Some(0.07),
            shape: Shape::Box {
                size: [1.1, 1.1, 1.1],
                bevel: 0.12,
                segments: 4,
                band: Some(Band {
                    height: 0.18,
                    overhang: 0.04
                }),
                foot: None,
            },
        }
    );
}

#[test]
fn shading_is_smooth_with_an_angle_or_flat() {
    let rock = |shading: &str| {
        parse(&format!(
            "kind = \"rock\"\nrole = \"rocks\"\nsize = [1, 1, 1]\n{shading}"
        ))
    };
    assert_eq!(
        rock("smooth_angle = 40").unwrap().shading,
        Shading::Smooth { angle: 40.0 }
    );
    assert_eq!(rock("shading = \"flat\"").unwrap().shading, Shading::Flat);
    assert!(rock("shading = \"flat\"\nsmooth_angle = 40").is_err());
    assert!(rock("shading = \"glossy\"").is_err());
}

#[test]
fn an_unknown_role_lists_the_palettes_roles() {
    let err = parse("kind = \"rock\"\nrole = \"lava\"\nsize = [1, 1, 1]").unwrap_err();
    assert!(err.contains("unknown role \"lava\""), "{err}");
    assert!(err.contains("crates"), "{err}");
}

#[test]
fn unknown_keys_and_kinds_are_errors() {
    // `jitter` belongs to rocks, not boxes.
    assert!(parse("kind = \"box\"\nrole = \"crates\"\nsize = [1, 1, 1]\njitter = 0.1").is_err());
    assert!(parse("kind = \"torus\"\nrole = \"crates\"").is_err());
    assert!(
        parse("kind = \"box\"\nrole = \"crates\"\nsize = [1, 1, 1]\ncolour = \"#fff\"").is_err()
    );
}

#[test]
fn a_bevel_must_fit_the_box() {
    let crate_with = |bevel: f64| {
        parse(&format!(
            "kind = \"box\"\nrole = \"crates\"\nsize = [1.0, 0.6, 1.0]\nbevel = {bevel}"
        ))
    };
    assert!(crate_with(0.29).is_ok());
    assert!(crate_with(0.3).is_err());
    assert!(crate_with(-0.1).is_err());
}

const IMPORT: &str = r#"
kind = "import"
role = "barriers"
source = "../sources/wall-low.glb"
scale = 1.15
credit = { title = "Wall (low)", author = "Kenney", licence = "LICENCE", url = "https://example.com/wall" }
"#;

#[test]
fn an_import_resolves_its_source_beside_the_file() {
    let model = parse(&IMPORT.replace("LICENCE", "CC0-1.0")).unwrap();
    let Shape::Import { source, scale, .. } = &model.shape else {
        panic!("{model:?}")
    };
    assert_eq!(source, &Path::new("models").join("../sources/wall-low.glb"));
    assert_eq!(*scale, 1.15);
}

#[test]
fn an_import_must_be_cc0_or_cc_by() {
    assert!(parse(&IMPORT.replace("LICENCE", "CC-BY-4.0")).is_ok());
    let err = parse(&IMPORT.replace("LICENCE", "CC-BY-NC-4.0")).unwrap_err();
    assert!(err.contains("not allowed"), "{err}");
    // A credit without a licence fails to parse.
    assert!(parse(&IMPORT.replace("licence = \"LICENCE\", ", "")).is_err());
}

#[test]
fn model_names_are_lowercase_identifiers() {
    let text = "kind = \"rock\"\nrole = \"rocks\"\nsize = [1, 1, 1]";
    assert!(Model::parse("rock_a2", text, Path::new("")).is_ok());
    assert!(Model::parse("Rock A", text, Path::new("")).is_err());
}
