//! Geometry contracts for the concept-sheet kit: wall joins and markings
//! must work from the game's camera, independently of close-up screenshots.
use std::{collections::BTreeSet, path::Path};

use calyx_model_gen::{
    obj::ObjMesh,
    params::{Model, Shape},
};

fn sandbox() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sandbox")
}

#[test]
fn every_tileable_wall_open_end_has_the_same_exported_contact_profile() {
    let mut expected = None;
    for (name, ports) in [
        ("wall_short", vec![(0, -1.0), (0, 1.0)]),
        ("wall_long", vec![(0, -2.0), (0, 2.0)]),
        ("wall_corner", vec![(0, 3.0), (2, 3.0)]),
        ("wall_t", vec![(0, -3.0), (0, 3.0), (2, 3.0)]),
        ("wall_cross", vec![(0, -3.0), (0, 3.0), (2, -3.0), (2, 3.0)]),
        ("wall_end", vec![(0, 1.0)]),
    ] {
        let file = sandbox().join(format!("assets/generated/models/{name}__body.obj"));
        let mesh = ObjMesh::parse(&std::fs::read_to_string(file).unwrap()).unwrap();
        for (axis, port) in ports {
            let across = if axis == 0 { 2 } else { 0 };
            let profile: BTreeSet<_> = mesh
                .positions
                .iter()
                .filter(|p| (p[axis] - port).abs() < 1e-4)
                .map(|p| {
                    (
                        (p[1] * 10000.0).round() as i64,
                        (p[across] * 10000.0).round() as i64,
                    )
                })
                .collect();
            assert!(profile.len() >= 4, "{name} needs a real flat contact face");
            if let Some(expected) = &expected {
                assert_eq!(&profile, expected, "{name}, axis {axis}, port {port}");
            } else {
                expected = Some(profile);
            }
        }
    }
}

#[test]
fn hazard_stripes_cover_the_camera_visible_top_instead_of_only_vertical_faces() {
    let model = Model::load(&sandbox().join("asset_sources/models/barrier_hazard.toml")).unwrap();
    let Shape::Parts { parts } = model.shape else {
        panic!("multi-material barrier")
    };
    let mut yellow_area = 0.0;
    let mut black_area = 0.0;
    for part in parts.iter().filter(|p| p.name.starts_with("top_")) {
        assert_eq!(part.rotation, [0.0; 3]);
        assert!(
            part.offset[1] >= 1.05,
            "markings must be above the concrete top"
        );
        let Shape::Prism { points, .. } = &part.shape else {
            panic!("clipped paint polygon")
        };
        let area = points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
            .sum::<f64>()
            .abs()
            / 2.0;
        match part.role.as_str() {
            "hazard_yellow" => yellow_area += area,
            "hazard_black" => black_area += area,
            other => panic!("unexpected paint role {other}"),
        }
    }
    assert!(
        yellow_area > 0.8,
        "a broad top marking must read at Far framing"
    );
    assert!(
        (0.35..0.65).contains(&(black_area / yellow_area)),
        "clear alternating dark/light bands"
    );
}

#[test]
fn extruded_footprints_and_fused_canopies_reject_invalid_dimensions() {
    for text in [
        "kind='prism'\nrole='walls'\npoints=[[0,0],[1,0],[2,0]]\nheight=1",
        "kind='prism'\nrole='walls'\npoints=[[0,0],[1,0],[0,1]]\nheight=-1",
        "kind='blob'\nrole='tree_canopies'\nlobes=[{center=[0,1,0],radius=1}]\nfuse=-0.1",
        "kind='cylinder'\nrole='tree_trunks'\nradius=0.2\ntop_radius=0\nheight=1",
    ] {
        assert!(
            Model::parse("invalid", text, Path::new("")).is_err(),
            "{text}"
        );
    }
}

#[test]
fn long_wall_replaces_two_short_walls_on_the_documented_root_grid() {
    let extent = |name: &str| {
        let mesh = ObjMesh::parse(
            &std::fs::read_to_string(
                sandbox().join(format!("assets/generated/models/{name}__body.obj")),
            )
            .unwrap(),
        )
        .unwrap();
        let min = mesh
            .positions
            .iter()
            .map(|p| p[0] as f64)
            .fold(f64::INFINITY, f64::min);
        let max = mesh
            .positions
            .iter()
            .map(|p| p[0] as f64)
            .fold(f64::NEG_INFINITY, f64::max);
        (min, max)
    };
    let short = extent("wall_short");
    let long = extent("wall_long");
    assert!(
        (short.1 - (2.0 + short.0)).abs() < 1e-4,
        "adjacent short faces meet"
    );
    assert!((short.0 - (1.0 + long.0)).abs() < 1e-4);
    assert!((2.0 + short.1 - (1.0 + long.1)).abs() < 1e-4);
    // Mixed short/long/junction chain roots remain on the one-metre lattice.
    let junction_port = 3.0;
    let short_root = junction_port - short.0;
    let long_root = short_root + short.1 - long.0;
    assert!((long_root + long.0 - (short_root + short.1)).abs() < 1e-4);
    for root in [short_root, long_root] {
        assert!((root - root.round()).abs() < 1e-4);
    }
}
