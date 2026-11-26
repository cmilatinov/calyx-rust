use super::*;

#[test]
fn normal_settings_read_the_meta_and_treat_none_as_calculate() {
    let meta = |mode: &str| {
        format!("ModelImporter:\n  tangentSpace:\n    normalSmoothAngle: 30\n    normalImportMode: {mode}\n")
    };
    assert_eq!(normal_settings(&meta("1")), (NORMALS_CALCULATE, 30.0));
    assert_eq!(normal_settings(&meta("2")), (NORMALS_CALCULATE, 30.0));
    assert_eq!(normal_settings(&meta("0")), (NORMALS_IMPORT, 30.0));
    assert_eq!(
        normal_settings("fileFormatVersion: 2\n"),
        (NORMALS_IMPORT, 60.0)
    );
}

/// Two triangles of a roof ridge, meeting at the ridge edge.
const RIDGE_POSITIONS: &str = "v 0 0 0\nv 1 1 0\nv 0 0 1\nv 2 0 0\nv 1 1 1\n";

#[test]
fn a_faceted_mesh_passes() {
    // Each triangle carries its own face normal.
    let obj = format!(
        "{RIDGE_POSITIONS}vn -0.7071 0.7071 0\nvn 0.7071 0.7071 0\nf 1//1 3//1 2//1\nf 4//2 2//2 5//2\n"
    );
    assert_eq!(check_faceted(&obj), Ok(()));
}

#[test]
fn a_smooth_mesh_fails() {
    // The ridge corners share one averaged normal.
    let obj = format!(
        "{RIDGE_POSITIONS}vn -0.7071 0.7071 0\nvn 0.7071 0.7071 0\nvn 0 1 0\nf 1//1 3//1 2//3\nf 4//2 2//3 5//2\n"
    );
    assert!(check_faceted(&obj).is_err());
}

#[test]
fn a_normal_against_the_winding_fails() {
    let obj = format!("{RIDGE_POSITIONS}vn 0.7071 -0.7071 0\nf 1//1 3//1 2//1\n");
    assert!(check_faceted(&obj).is_err());
}
