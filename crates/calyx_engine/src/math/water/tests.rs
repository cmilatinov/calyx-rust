use super::*;
use crate::math::{Heightfield, HeightfieldExtent};
use approx::assert_abs_diff_eq;

fn point(x: f32, z: f32, width: f32) -> WaterPathPoint {
    WaterPathPoint {
        position: vec2(x, z),
        width,
    }
}

/// A straight river 4 wide and 10 long along +Z.
fn straight() -> WaterPath {
    WaterPath {
        points: vec![point(0.0, 0.0, 4.0), point(0.0, 10.0, 4.0)],
        cell_size: 1.0,
        obstacles: Vec::new(),
        waterlines: Vec::new(),
        openings: Vec::new(),
    }
}

/// A river that bends left and right, narrowing and widening.
fn meander() -> WaterPath {
    WaterPath {
        points: vec![
            point(0.0, 0.0, 6.0),
            point(5.0, 12.0, 8.0),
            point(-3.0, 24.0, 5.0),
            point(2.0, 36.0, 7.0),
        ],
        cell_size: 0.8,
        obstacles: vec![WaterCircle {
            center: vec2(5.0, 12.0),
            radius: 1.0,
        }],
        waterlines: Vec::new(),
        openings: Vec::new(),
    }
}

/// Twice the signed area of a triangle seen from above, in the X, Z
/// plane: positive when it runs counter-clockwise.
fn winding(a: Vec3, b: Vec3, c: Vec3) -> f32 {
    let (u, v) = (b - a, c - a);
    u.x * v.z - u.z * v.x
}

#[test]
fn a_path_needs_two_points_a_width_and_a_cell_size() {
    assert!(straight().is_valid());
    let mut path = straight();
    path.points.pop();
    assert!(!path.is_valid());
    let mut path = straight();
    path.points[1].width = 0.0;
    assert!(path.geometry().is_none());
    let mut path = straight();
    path.cell_size = 0.0;
    assert!(path.sections().is_none());
    let mut path = straight();
    path.points[1].position = path.points[0].position;
    assert!(!path.is_valid());
}

#[test]
fn a_straight_river_meshes_rows_along_it_and_columns_across() {
    let geometry = straight().geometry().unwrap();
    // 10 rows along, 4 columns across, two triangles of three vertices.
    let vertices = 10 * 4 * 6;
    assert_eq!(geometry.positions.len(), vertices);
    assert_eq!(geometry.indices.len(), vertices);
    for len in [
        geometry.river.len(),
        geometry.next.len(),
        geometry.previous.len(),
        geometry.next_river.len(),
        geometry.extra.len(),
        geometry.facets.len(),
        geometry.footprint.len(),
    ] {
        assert_eq!(len, vertices);
    }
    assert!(geometry
        .indices
        .iter()
        .enumerate()
        .all(|(i, index)| *index as usize == i));
    for (position, river) in geometry.positions.iter().zip(&geometry.river) {
        assert_eq!(position.y, 0.0);
        // Flowing along +Z, the left of the flow is -X.
        assert_abs_diff_eq!(river.x, position.z, epsilon = 1e-4);
        assert_abs_diff_eq!(river.y, -position.x, epsilon = 1e-4);
        assert!(position.x.abs() <= 2.0 + 1e-4);
    }
}

#[test]
fn triangles_face_up_as_a_heightfields_do() {
    let geometry = meander().geometry().unwrap();
    for corners in geometry.positions.chunks(3) {
        assert!(winding(corners[0], corners[1], corners[2]) > 0.0);
    }
    let field = Heightfield::new(2, 2, vec![0.0; 4]).unwrap();
    let terrain = field
        .geometry(&HeightfieldExtent {
            size: vec2(1.0, 1.0),
            height_scale: 1.0,
        })
        .unwrap();
    let corner = |i: usize| terrain.positions[terrain.indices[i] as usize];
    assert!(winding(corner(0), corner(1), corner(2)) > 0.0);
}

#[test]
fn each_vertex_knows_its_triangles_other_corners_in_drawn_order() {
    let geometry = meander().geometry().unwrap();
    let xz = |p: Vec3| vec2(p.x, p.z);
    for t in 0..geometry.positions.len() / 3 {
        for i in 0..3 {
            let vertex = t * 3 + i;
            let next = t * 3 + (i + 1) % 3;
            let previous = t * 3 + (i + 2) % 3;
            assert_eq!(geometry.next[vertex], xz(geometry.positions[next]));
            assert_eq!(geometry.previous[vertex], xz(geometry.positions[previous]));
            assert_eq!(geometry.next_river[vertex], geometry.river[next]);
            let extra = geometry.extra[vertex];
            assert_eq!(vec2(extra.x, extra.y), geometry.river[previous]);
            assert_eq!(geometry.facets[vertex], geometry.facets[t * 3]);
            // One footprint per facet: its extents and rounding.
            let (own, first) = (geometry.footprint[vertex], geometry.footprint[t * 3]);
            assert_eq!(vec2(own.z, own.w), vec2(first.z, first.w));
            assert_eq!(extra.w, geometry.extra[t * 3].w);
        }
    }
}

/// Where a river opens into other water, its shore there is open
/// water: no bank is cut along it, and the water there lies as far from
/// the shore as the nearest stretch of shore that meets land, so no
/// shallows or foam band draws across the mouth.
#[test]
fn an_opening_leaves_its_shore_without_a_bank_or_a_shore() {
    let profile = BankProfile {
        height: 0.6,
        depth: 0.6,
        run: 0.4,
        lip: 0.5,
    };
    let mut path = straight();
    let whole = path.banks(&profile).unwrap().positions.len();
    // An opening on the left shore (-X) halfway along.
    path.openings.push(WaterCircle {
        center: vec2(-2.0, 5.0),
        radius: 2.0,
    });
    let banks = path.banks(&profile).unwrap();
    assert!(banks.positions.len() < whole);
    assert!(banks
        .positions
        .iter()
        .all(|p| p.x > 0.0 || (p.z - 5.0).abs() >= 1.0 - 1e-4));
    let geometry = path.geometry().unwrap();
    let mut across_the_mouth = 0;
    for (position, extra) in geometry.positions.iter().zip(&geometry.extra) {
        let own = 2.0 - position.x.abs();
        let open = position.x < 0.0 && (position.z - 5.0).abs() < 2.0;
        if !open {
            assert_abs_diff_eq!(extra.z, own, epsilon = 1e-4);
        } else {
            // Never nearer the shore than its own, and well clear of it
            // in the mouth's middle, where the nearest land is the
            // mouth's ends or the far shore.
            assert!(extra.z >= own - 1e-4);
            if (position.z - 5.0).abs() < 0.5 {
                across_the_mouth += 1;
                assert!(extra.z > 1.4, "{position:?} is {} from the shore", extra.z);
            }
        }
    }
    assert!(across_the_mouth > 0);
}

#[test]
fn shore_distance_runs_from_zero_at_the_banks_to_half_the_width() {
    let geometry = straight().geometry().unwrap();
    for (river, extra) in geometry.river.iter().zip(&geometry.extra) {
        assert_abs_diff_eq!(extra.z, 2.0 - river.y.abs(), epsilon = 1e-4);
    }
    let shores: Vec<f32> = geometry.extra.iter().map(|extra| extra.z).collect();
    assert_eq!(shores.iter().copied().fold(f32::MAX, f32::min), 0.0);
    assert!(shores.iter().copied().fold(0.0, f32::max) <= 2.0);
    for (corners, facet) in geometry
        .extra
        .chunks(3)
        .zip(geometry.facets.iter().step_by(3))
    {
        let centroid = (corners[0].z + corners[1].z + corners[2].z) / 3.0;
        assert_abs_diff_eq!(facet.y, centroid, epsilon = 1e-5);
        assert!((0.0..1.0).contains(&facet.x));
    }
}

#[test]
fn a_meander_follows_its_points_and_widths() {
    let path = meander();
    let sections = path.sections().unwrap();
    // Through every control point, as wide as it is there.
    for control in &path.points {
        let nearest = sections
            .iter()
            .min_by(|a, b| {
                (a.center - control.position)
                    .norm()
                    .total_cmp(&(b.center - control.position).norm())
            })
            .unwrap();
        assert!((nearest.center - control.position).norm() < path.cell_size);
        assert_abs_diff_eq!(nearest.half_width * 2.0, control.width, epsilon = 0.3);
    }
    // Rows are evenly spaced along the curve and flow from start to end.
    let step = sections[1].s;
    assert!((step - path.cell_size).abs() < 0.1);
    for pair in sections.windows(2) {
        assert_abs_diff_eq!(pair[1].s - pair[0].s, step, epsilon = 1e-3);
        assert!((pair[1].center - pair[0].center).norm() <= step + 1e-3);
        assert_abs_diff_eq!(pair[0].tangent.norm(), 1.0, epsilon = 1e-4);
    }
    // It bends one way, then the other.
    let turns: Vec<f32> = sections
        .windows(2)
        .map(|pair| pair[0].tangent.x * pair[1].tangent.y - pair[0].tangent.y * pair[1].tangent.x)
        .collect();
    assert!(turns.iter().any(|turn| *turn > 1e-3));
    assert!(turns.iter().any(|turn| *turn < -1e-3));
}

/// The distance to a facet's footprint, as the shader measures it per
/// pixel from the vertex's footprint channels at `vertex`.
fn footprint_distance(geometry: &WaterGeometry, vertex: usize) -> f32 {
    let footprint = geometry.footprint[vertex];
    let rounding = geometry.extra[vertex].w;
    if rounding < 0.0 {
        return NO_OBSTACLE;
    }
    crate::math::rounded_box_distance(
        vec2(footprint.x, footprint.y),
        vec2(footprint.z, footprint.w),
        rounding,
    )
}

#[test]
fn facets_near_an_obstacle_measure_their_distance_to_it_and_the_rest_have_none() {
    let path = meander();
    let footprints = path.footprints().unwrap();
    assert_eq!(footprints.len(), 1);
    let geometry = path.geometry().unwrap();
    let mut near = 0;
    for (vertex, position) in geometry.positions.iter().enumerate() {
        let own = (vec2(position.x, position.z) - vec2(5.0, 12.0)).norm() - 1.0;
        let measured = footprint_distance(&geometry, vertex);
        if measured == NO_OBSTACLE {
            // Out of reach: its facet's centroid is.
            assert!(own > MAX_OBSTACLE_REACH - path.cell_size * 2.0, "{own}");
            assert_eq!(geometry.facets[vertex].z, NO_OBSTACLE);
        } else {
            near += 1;
            assert_abs_diff_eq!(measured, own, epsilon = 0.02);
            assert!(geometry.facets[vertex].z < MAX_OBSTACLE_REACH);
        }
    }
    assert!(near > 0 && near < geometry.positions.len());
    // Nothing in the water, nothing to ring.
    let geometry = straight().geometry().unwrap();
    assert!(geometry.extra.iter().all(|extra| extra.w == -1.0));
    assert!(geometry.facets.iter().all(|facet| facet.z == NO_OBSTACLE));
}

/// The shore the bank foam and shallows run from is the same with
/// footprints in the water as without.
#[test]
fn footprints_leave_the_shore_distance_as_it_was() {
    let mut path = straight();
    path.openings.push(WaterCircle {
        center: vec2(-2.0, 5.0),
        radius: 2.0,
    });
    let before = path.geometry().unwrap();
    path.obstacles.push(WaterCircle {
        center: vec2(1.0, 3.0),
        radius: 0.5,
    });
    path.waterlines
        .push(WaterFootprint::circle(vec2(-1.0, 7.0), 0.4).outline());
    let after = path.geometry().unwrap();
    assert_eq!(before.positions, after.positions);
    for (a, b) in before.extra.iter().zip(&after.extra) {
        assert_eq!(a.z, b.z);
    }
    for (a, b) in before.facets.iter().zip(&after.facets) {
        assert_eq!((a.x, a.y), (b.x, b.y));
    }
    assert_eq!(path.footprints().unwrap().len(), 2);
}

/// A waterline counts where some of it lies over the water and it is no
/// wider than a footprint can be; an authored obstacle and a waterline
/// around the same rock ring once.
#[test]
fn waterlines_over_the_water_become_footprints_and_others_do_not() {
    let mut path = straight();
    // A rock in the river, a circle authored around its middle, a rock
    // on the far bank, and the ground's cut along the whole shore.
    let rock = WaterFootprint {
        center: vec2(0.5, 4.0),
        axis: vec2(1.0, 0.0),
        half: vec2(0.7, 0.6),
        rounding: 0.3,
    };
    path.waterlines.push(rock.outline());
    path.obstacles.push(WaterCircle {
        center: vec2(0.5, 4.0),
        radius: 0.5,
    });
    path.waterlines
        .push(WaterFootprint::circle(vec2(4.0, 5.0), 0.5).outline());
    path.waterlines.push(vec![
        vec2(2.1, -1.0),
        vec2(2.1, 11.0),
        vec2(1.9, 11.0),
        vec2(1.9, -1.0),
    ]);
    let footprints = path.footprints().unwrap();
    assert_eq!(footprints.len(), 1, "{footprints:?}");
    assert_abs_diff_eq!(footprints[0].center, rock.center, epsilon = 1e-3);
    assert_abs_diff_eq!(footprints[0].half.max(), 0.7, epsilon = 1e-3);
}

#[test]
fn the_region_spans_the_water_from_shore_to_shore() {
    let (least, greatest) = water_region(&straight().sections().unwrap());
    assert_abs_diff_eq!(least, vec2(-2.0, 0.0), epsilon = 1e-4);
    assert_abs_diff_eq!(greatest, vec2(2.0, 10.0), epsilon = 1e-4);
}

#[test]
fn banks_rise_from_under_the_water_to_a_lip_on_both_shores() {
    let profile = BankProfile {
        height: 0.6,
        depth: 0.4,
        run: 0.5,
        lip: 0.5,
    };
    let banks = straight().banks(&profile).unwrap();
    // 10 rows, 2 strips (wall and lip), 2 sides, 2 triangles of 3.
    assert_eq!(banks.positions.len(), 10 * 2 * 2 * 6);
    assert_eq!(banks.normals.len(), banks.positions.len());
    let ys: Vec<f32> = banks.positions.iter().map(|p| p.y).collect();
    assert_abs_diff_eq!(ys.iter().copied().fold(f32::MAX, f32::min), -0.4);
    assert_abs_diff_eq!(ys.iter().copied().fold(f32::MIN, f32::max), 0.6);
    // The wall's foot is inside the shore, X = +-2, its top outside.
    let xs: Vec<f32> = banks.positions.iter().map(|p| p.x.abs()).collect();
    assert!(xs.iter().copied().fold(f32::MAX, f32::min) < 2.0);
    assert_abs_diff_eq!(
        xs.iter().copied().fold(0.0, f32::max),
        2.0 + 0.3 + 0.5,
        epsilon = 1e-4
    );
    for (corners, normals) in banks.positions.chunks(3).zip(banks.normals.chunks(3)) {
        let normal = normals[0];
        // Every face looks up, and a wall towards the river (X = 0).
        assert!(normal.y > 0.0);
        let center = (corners[0] + corners[1] + corners[2]) / 3.0;
        assert!(normal.x * center.x <= 1e-6);
        // Its winding agrees with its normal, as the terrain's does.
        let facing = (corners[2] - corners[0]).cross(&(corners[1] - corners[0]));
        assert!(facing.dot(&normal) > 0.0);
    }
    assert!(straight()
        .banks(&BankProfile {
            height: 0.0,
            depth: 0.0,
            ..profile
        })
        .is_none());
}
