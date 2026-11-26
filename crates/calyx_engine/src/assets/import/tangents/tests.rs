use nalgebra_glm::{Vec2, Vec3, Vec4};

use super::generate;
use crate::assets::import::{ImportedBone, ImportedMesh};

/// A unit quad in the XY plane facing +Z, with `uvs` at its corners (bottom left, bottom
/// right, top right, top left), as two triangles in the renderer's winding.
fn quad(uvs: [Vec2; 4]) -> ImportedMesh {
    ImportedMesh {
        name: "quad".into(),
        positions: vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ],
        normals: vec![Vec3::z(); 4],
        uvs: vec![uvs.to_vec()],
        tangents: Vec::new(),
        indices: vec![0, 2, 1, 0, 3, 2],
        bones: Vec::new(),
        material: None,
    }
}

fn assert_near(actual: Vec4, expected: Vec4) {
    assert!(
        (actual - expected).norm() < 1e-5,
        "expected {expected:?}, got {actual:?}"
    );
}

#[test]
fn a_quad_without_tangents_gets_them_along_u_with_the_bitangent_up_the_image() {
    // The image upright on the quad: V grows downwards, as the renderer samples it.
    let mut mesh = quad([
        Vec2::new(0.0, 1.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(0.0, 0.0),
    ]);

    generate(&mut mesh);

    assert_eq!(mesh.positions.len(), 4, "no vertex is split");
    for tangent in &mesh.tangents {
        assert_near(*tangent, Vec4::new(1.0, 0.0, 0.0, 1.0));
        // The bitangent, cross(normal, tangent) * w, points up the image: +Y.
        let bitangent = Vec3::z().cross(&tangent.xyz()) * tangent.w;
        assert!((bitangent - Vec3::y()).norm() < 1e-5);
    }
}

#[test]
fn a_mirrored_image_flips_the_handedness() {
    // The image mirrored left to right: U grows towards -X.
    let mut mesh = quad([
        Vec2::new(1.0, 1.0),
        Vec2::new(0.0, 1.0),
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 0.0),
    ]);

    generate(&mut mesh);

    for tangent in &mesh.tangents {
        assert_near(*tangent, Vec4::new(-1.0, 0.0, 0.0, -1.0));
    }
}

#[test]
fn a_vertex_shared_by_both_handednesses_is_split_with_its_skin_weights() {
    // Two quads sharing the edge x = 1, the second with its image mirrored about it.
    let mut mesh = ImportedMesh {
        name: "mirrored".into(),
        positions: vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(2.0, 1.0, 0.0),
        ],
        normals: vec![Vec3::z(); 6],
        uvs: vec![vec![
            Vec2::new(0.0, 1.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, 0.0),
            Vec2::new(0.0, 1.0),
            Vec2::new(0.0, 0.0),
        ]],
        tangents: Vec::new(),
        indices: vec![0, 2, 1, 0, 3, 2, 1, 5, 4, 1, 2, 5],
        bones: vec![ImportedBone {
            name: "bone".into(),
            inverse_bind_transform: Default::default(),
            weights: vec![(1, 0.5), (2, 1.0)],
        }],
        material: None,
    };

    generate(&mut mesh);

    assert_eq!(mesh.positions.len(), 8, "both seam vertices are split");
    assert_eq!(mesh.tangents.len(), 8);
    for (index, &vertex) in mesh.indices.iter().enumerate() {
        let expected = if index < 6 { 1.0 } else { -1.0 };
        assert_eq!(mesh.tangents[vertex as usize].w, expected);
    }
    let mut weights = mesh.bones[0].weights.clone();
    weights.sort_by_key(|(vertex, _)| *vertex);
    assert_eq!(weights, vec![(1, 0.5), (2, 1.0), (6, 0.5), (7, 1.0)]);
}

#[test]
fn a_mesh_without_uvs_gets_tangents_perpendicular_to_its_normals() {
    let mut mesh = quad([Vec2::zeros(); 4]);
    mesh.uvs.clear();

    generate(&mut mesh);

    assert_eq!(mesh.tangents.len(), 4);
    for tangent in &mesh.tangents {
        assert!(tangent.xyz().dot(&Vec3::z()).abs() < 1e-6);
        assert!((tangent.xyz().norm() - 1.0).abs() < 1e-6);
    }
}
