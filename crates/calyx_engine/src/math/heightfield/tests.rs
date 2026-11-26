use super::*;
use approx::assert_abs_diff_eq;
use image::{ImageBuffer, Luma};

fn extent(size: f32, height_scale: f32) -> HeightfieldExtent {
    HeightfieldExtent {
        size: vec2(size, size),
        height_scale,
    }
}

/// Heights of the plane h = 0.1 * column + 0.05 * row.
fn ramp(columns: usize, rows: usize) -> Heightfield {
    let heights = (0..rows)
        .flat_map(|row| (0..columns).map(move |column| 0.1 * column as f32 + 0.05 * row as f32))
        .collect();
    Heightfield::new(columns, rows, heights).unwrap()
}

#[test]
fn sides_must_be_at_least_two_samples_and_heights_finite() {
    assert!(Heightfield::new(1, 4, vec![0.0; 4]).is_none());
    assert!(Heightfield::new(2, 2, vec![0.0; 3]).is_none());
    assert!(Heightfield::new(2, 2, vec![0.0, f32::NAN, 0.0, 0.0]).is_none());
    let too_wide = MAX_HEIGHTFIELD_SAMPLES + 1;
    assert!(Heightfield::new(too_wide, 2, vec![0.0; too_wide * 2]).is_none());
    assert!(Heightfield::new(2, 2, vec![0.0; 4]).is_some());
}

#[test]
fn a_sixteen_bit_image_keeps_its_precision_and_rows_run_from_its_top() {
    let image = ImageBuffer::from_fn(3, 2, |x, y| Luma([(x * 1000 + y * 30000 + 1) as u16]));
    let field = Heightfield::from_image(&DynamicImage::ImageLuma16(image)).unwrap();
    assert_eq!((field.columns(), field.rows()), (3, 2));
    assert_abs_diff_eq!(field.height(0, 0), 1.0 / 65535.0, epsilon = 1e-7);
    assert_abs_diff_eq!(field.height(2, 1), 32001.0 / 65535.0, epsilon = 1e-7);
    // Row 0 is the image's top, on the +Z edge.
    let sample = field.sample(&extent(2.0, 1.0), vec2(-1.0, 1.0)).unwrap();
    assert_abs_diff_eq!(sample.height, field.height(0, 0), epsilon = 1e-6);
}

#[test]
fn an_oversized_image_is_resampled_end_to_end() {
    let width = MAX_HEIGHTFIELD_SAMPLES as u32 * 2;
    let image = ImageBuffer::from_fn(width, 2, |x, _| {
        Luma([(x as f32 / (width - 1) as f32 * 65535.0).round() as u16])
    });
    let field = Heightfield::from_image(&DynamicImage::ImageLuma16(image)).unwrap();
    assert_eq!(field.columns(), MAX_HEIGHTFIELD_SAMPLES);
    assert_abs_diff_eq!(field.height(0, 0), 0.0, epsilon = 1e-6);
    assert_abs_diff_eq!(
        field.height(MAX_HEIGHTFIELD_SAMPLES - 1, 1),
        1.0,
        epsilon = 1e-6
    );
    assert_abs_diff_eq!(
        field.height(MAX_HEIGHTFIELD_SAMPLES / 2, 0),
        (MAX_HEIGHTFIELD_SAMPLES / 2) as f32 / (MAX_HEIGHTFIELD_SAMPLES - 1) as f32,
        epsilon = 2e-5
    );
    assert!(Heightfield::from_image(&DynamicImage::new_luma8(1, 8)).is_none());
}

#[test]
fn a_plane_samples_its_height_and_normal_everywhere() {
    // 5 x 5 samples over 8 x 8 units: 2 units apart. Height rises 0.1 per
    // column (+X) and 0.05 per row (-Z), scaled by 4.
    let field = ramp(5, 5);
    let extent = extent(8.0, 4.0);
    let slope_x = 0.1 * 4.0 / 2.0;
    let slope_z = -0.05 * 4.0 / 2.0;
    let expected_normal = glm::normalize(&vec3(-slope_x, 1.0, -slope_z));
    for (x, z) in [
        (-4.0, 4.0),
        (0.3, -1.7),
        (3.9, -3.9),
        (4.0, -4.0),
        (-2.5, 0.75),
    ] {
        let sample = field.sample(&extent, vec2(x, z)).unwrap();
        let expected = (0.1 * (x + 4.0) / 2.0 + 0.05 * (4.0 - z) / 2.0) * 4.0;
        assert_abs_diff_eq!(sample.height, expected, epsilon = 1e-5);
        assert_abs_diff_eq!(sample.normal, expected_normal, epsilon = 1e-5);
    }
}

#[test]
fn samples_follow_the_drawn_triangles_and_are_none_outside() {
    // One raised corner: (column 1, row 1), the +X, -Z corner. The cell's
    // diagonal runs from (1, 0) to (0, 1), so the a-c-b triangle stays
    // flat and only the b-c-d triangle rises.
    let field = Heightfield::new(2, 2, vec![0.0, 0.0, 0.0, 1.0]).unwrap();
    let extent = extent(2.0, 1.0);
    let flat = field.sample(&extent, vec2(-0.5, 0.5)).unwrap();
    assert_abs_diff_eq!(flat.height, 0.0, epsilon = 1e-6);
    let raised = field.sample(&extent, vec2(0.5, -0.5)).unwrap();
    assert_abs_diff_eq!(raised.height, 0.5, epsilon = 1e-6);
    let corner = field.sample(&extent, vec2(1.0, -1.0)).unwrap();
    assert_abs_diff_eq!(corner.height, 1.0, epsilon = 1e-6);
    // On the diagonal both triangles agree.
    let diagonal = field.sample(&extent, vec2(0.0, 0.0)).unwrap();
    assert_abs_diff_eq!(diagonal.height, 0.0, epsilon = 1e-6);
    assert!(raised.normal.y > 0.0 && raised.normal.x < 0.0 && raised.normal.z > 0.0);

    for outside in [vec2(1.01, 0.0), vec2(0.0, -1.01), vec2(f32::NAN, 0.0)] {
        assert!(field.sample(&extent, outside).is_none());
    }
    let invalid = HeightfieldExtent {
        size: vec2(0.0, 2.0),
        height_scale: 1.0,
    };
    assert!(field.sample(&invalid, Vec2::zeros()).is_none());
    assert!(field.geometry(&invalid).is_none());
}

#[test]
fn geometry_has_a_vertex_per_sample_and_upward_counter_clockwise_triangles() {
    let field = ramp(4, 3);
    let extent = HeightfieldExtent {
        size: vec2(6.0, 4.0),
        height_scale: 2.0,
    };
    let geometry = field.geometry(&extent).unwrap();
    assert_eq!(geometry.positions.len(), 12);
    assert_eq!(geometry.normals.len(), 12);
    assert_eq!(geometry.tangents.len(), 12);
    assert_eq!(geometry.uvs.len(), 12);
    assert_eq!(geometry.indices.len(), 3 * 2 * 6);
    assert_eq!(geometry.positions[0], vec3(-3.0, 0.0, 2.0));
    assert_abs_diff_eq!(
        geometry.positions[11],
        vec3(3.0, (0.3 + 0.1) * 2.0, -2.0),
        epsilon = 1e-6
    );
    assert_eq!(geometry.uvs[0], vec2(0.0, 0.0));
    assert_eq!(geometry.uvs[11], vec2(1.0, 1.0));
    for triangle in geometry.indices.chunks(3) {
        let [a, b, c] = [0, 1, 2].map(|i| geometry.positions[triangle[i] as usize]);
        // Counter-clockwise seen from above, with X right and Z up the
        // screen: the 2D cross product of the XZ edges is positive.
        let (e1, e2) = (b - a, c - a);
        assert!(e1.x * e2.z - e1.z * e2.x > 0.0, "triangle {triangle:?}");
        // Every sample's height and normal lies on the drawn surface.
        for vertex in [a, b, c] {
            let sample = field.sample(&extent, vec2(vertex.x, vertex.z)).unwrap();
            assert_abs_diff_eq!(sample.height, vertex.y, epsilon = 1e-5);
        }
        let centroid = (a + b + c) / 3.0;
        let sample = field.sample(&extent, vec2(centroid.x, centroid.z)).unwrap();
        assert_abs_diff_eq!(sample.height, centroid.y, epsilon = 1e-5);
    }
    for (normal, tangent) in geometry.normals.iter().zip(&geometry.tangents) {
        assert_abs_diff_eq!(normal.norm(), 1.0, epsilon = 1e-5);
        assert!(normal.y > 0.0);
        let tangent = tangent.xyz();
        assert_abs_diff_eq!(tangent.norm(), 1.0, epsilon = 1e-5);
        assert_abs_diff_eq!(tangent.dot(normal), 0.0, epsilon = 1e-5);
    }
}

#[test]
fn a_heightmap_file_loads_at_full_precision() {
    let dir = std::env::temp_dir().join(format!("heightfield-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("height.png");
    ImageBuffer::from_fn(4, 4, |x, y| Luma([(x * 4 + y) as u16 * 4000]))
        .save(&path)
        .unwrap();
    let field = Heightfield::load(&path).unwrap();
    assert_eq!((field.columns(), field.rows()), (4, 4));
    assert_abs_diff_eq!(field.height(3, 2), 56000.0 / 65535.0, epsilon = 1e-7);
    assert!(Heightfield::load(&dir.join("missing.png")).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}
