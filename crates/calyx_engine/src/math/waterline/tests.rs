use super::*;
use approx::assert_abs_diff_eq;
use nalgebra_glm::{rotation, translation, vec3};

/// A closed box from `least` to `greatest`, as a mesh's positions and
/// triangles, each face its own four vertices, as a flat-shaded model's.
fn cuboid(least: Vec3, greatest: Vec3) -> (Vec<Vec3>, Vec<u32>) {
    let corner = |i: usize| {
        vec3(
            if i & 1 == 0 { least.x } else { greatest.x },
            if i & 2 == 0 { least.y } else { greatest.y },
            if i & 4 == 0 { least.z } else { greatest.z },
        )
    };
    let faces = [
        [0, 1, 3, 2],
        [4, 6, 7, 5],
        [0, 4, 5, 1],
        [2, 3, 7, 6],
        [0, 2, 6, 4],
        [1, 5, 7, 3],
    ];
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    for face in faces {
        let base = positions.len() as u32;
        positions.extend(face.map(corner));
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (positions, indices)
}

#[test]
fn a_box_crossing_the_surface_cuts_its_outline_and_one_above_cuts_none() {
    let (positions, indices) = cuboid(vec3(-1.0, -0.5, -0.25), vec3(1.0, 0.5, 0.25));
    let pieces = waterline(&positions, &indices, &Mat4::identity());
    assert_eq!(pieces.len(), 1, "one piece: {pieces:?}");
    let footprint = WaterFootprint::fit(&pieces[0]).unwrap();
    assert_abs_diff_eq!(footprint.center, Vec2::zeros(), epsilon = 1e-4);
    let half = footprint.half;
    // Its long side is along X either way round.
    let (long, short) = (half.max(), half.min());
    assert_abs_diff_eq!(long, 1.0, epsilon = 1e-4);
    assert_abs_diff_eq!(short, 0.25, epsilon = 1e-4);
    assert_abs_diff_eq!(footprint.rounding, 0.0, epsilon = 1e-2);
    // A deck standing clear of the surface, and one lying on it from
    // above, cut nothing.
    let above = translation(&vec3(0.0, 0.6, 0.0));
    assert!(waterline(&positions, &indices, &above).is_empty());
    let resting = translation(&vec3(0.0, 0.5, 0.0));
    assert!(waterline(&positions, &indices, &resting).is_empty());
}

#[test]
fn separate_pieces_of_one_mesh_cut_separate_outlines_in_the_waters_space() {
    let (mut positions, mut indices) = cuboid(vec3(-0.1, -1.0, -0.1), vec3(0.1, 1.0, 0.1));
    let (post, post_indices) = cuboid(vec3(2.9, -1.0, -0.1), vec3(3.1, 1.0, 0.1));
    let base = positions.len() as u32;
    positions.extend(post);
    indices.extend(post_indices.iter().map(|i| i + base));
    // Carried a metre along Z and turned a quarter about Y.
    let to_water =
        translation(&vec3(0.0, 0.0, 1.0)) * rotation(std::f32::consts::FRAC_PI_2, &Vec3::y());
    let pieces = waterline(&positions, &indices, &to_water);
    assert_eq!(pieces.len(), 2);
    let centers: Vec<Vec2> = pieces
        .iter()
        .map(|piece| WaterFootprint::fit(piece).unwrap().center)
        .collect();
    assert_abs_diff_eq!(centers[0], vec2(0.0, 1.0), epsilon = 1e-4);
    // X turns to -Z.
    assert_abs_diff_eq!(centers[1], vec2(0.0, -2.0), epsilon = 1e-4);
}

#[test]
fn a_circles_outline_fits_back_to_the_circle() {
    let circle = WaterFootprint::circle(vec2(2.0, -1.0), 0.75);
    assert_abs_diff_eq!(circle.distance(vec2(2.0, -1.0)), -0.75, epsilon = 1e-5);
    assert_abs_diff_eq!(circle.distance(vec2(4.0, -1.0)), 1.25, epsilon = 1e-5);
    let fitted = WaterFootprint::fit(&circle.outline()).unwrap();
    assert_abs_diff_eq!(fitted.center, circle.center, epsilon = 1e-3);
    for angle in [0.0f32, 0.4, 1.3, 2.9] {
        let p = circle.center + vec2(angle.cos(), angle.sin()) * 2.0;
        assert_abs_diff_eq!(fitted.distance(p), 1.25, epsilon = 0.03);
    }
}

#[test]
fn a_rounded_rectangles_distance_is_to_its_nearest_edge() {
    let footprint = WaterFootprint {
        center: vec2(1.0, 1.0),
        axis: vec2(0.0, 1.0),
        half: vec2(2.0, 1.0),
        rounding: 0.5,
    };
    // Along the axis (+Z) it reaches 2, across it (-X) 1.
    assert_abs_diff_eq!(footprint.distance(vec2(1.0, 4.0)), 1.0, epsilon = 1e-5);
    assert_abs_diff_eq!(footprint.distance(vec2(-1.0, 1.0)), 1.0, epsilon = 1e-5);
    assert_abs_diff_eq!(footprint.distance(vec2(1.0, 1.0)), -1.0, epsilon = 1e-5);
    // Past a corner, from its rounding's centre.
    let corner = vec2(1.0 - 0.5, 1.0 + 1.5);
    let p = corner + vec2(-1.0, 1.0).normalize() * 1.5;
    assert_abs_diff_eq!(footprint.distance(p), 1.0, epsilon = 1e-4);
}

#[test]
fn overlapping_footprints_merge_and_distant_ones_stay_apart() {
    let rock = WaterFootprint {
        center: vec2(0.0, 0.0),
        axis: vec2(1.0, 0.0),
        half: vec2(0.7, 0.6),
        rounding: 0.3,
    };
    let inside = WaterFootprint::circle(vec2(0.1, 0.0), 0.5);
    let far = WaterFootprint::circle(vec2(5.0, 0.0), 0.5);
    let merged = merge_footprints(vec![rock.outline(), inside.outline(), far.outline()]);
    assert_eq!(merged.len(), 2, "{merged:?}");
    let near = merged
        .iter()
        .find(|footprint| footprint.center.norm() < 1.0)
        .unwrap();
    // The circle lay inside the rock's rectangle, so the rectangle is
    // what is left.
    assert_abs_diff_eq!(near.half.max(), 0.7, epsilon = 1e-3);
    assert_abs_diff_eq!(near.half.min(), 0.6, epsilon = 1e-3);
    assert!(merged
        .iter()
        .any(|footprint| (footprint.center - far.center).norm() < 1e-3));
}

#[test]
fn a_hull_drops_inner_and_collinear_points() {
    let hull = convex_hull(&[
        vec2(0.0, 0.0),
        vec2(1.0, 0.0),
        vec2(2.0, 0.0),
        vec2(2.0, 2.0),
        vec2(1.0, 1.0),
        vec2(0.0, 2.0),
        vec2(f32::NAN, 0.0),
    ]);
    assert_eq!(hull.len(), 4);
    assert_abs_diff_eq!(polygon_area(&hull), 4.0);
    let line = convex_hull(&[vec2(0.0, 0.0), vec2(1.0, 1.0), vec2(2.0, 2.0)]);
    assert_eq!(line, vec![vec2(0.0, 0.0), vec2(2.0, 2.0)]);
}
