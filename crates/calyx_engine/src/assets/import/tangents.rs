//! Vertex tangent generation for meshes whose file carries none.
//!
//! The method follows MikkTSpace in the parts that decide how a normal map reads: each
//! triangle's tangent and bitangent are the directions of increasing U and of decreasing V
//! (up the image) on its surface, normalised, and each corner adds them weighted by the angle the
//! triangle makes there, projected into the plane of the corner's normal. Corners whose triangles
//! disagree on handedness, as on either side of a mirrored UV seam, never share a tangent: such a
//! vertex is split in two, one per handedness. Triangles without UV area add nothing, and a
//! vertex that gets no tangent at all takes any direction perpendicular to its normal.

use std::collections::HashMap;

use nalgebra_glm::{Vec2, Vec3, Vec4};

use super::ImportedMesh;

/// Below this, a triangle's UV area or a vector's length counts as zero.
const EPSILON: f32 = 1e-12;

/// Sums of the frames a vertex receives from the corners of one handedness.
#[derive(Default)]
struct Accumulated {
    tangent: Vec3,
    bitangent: Vec3,
}

/// Fills `mesh.tangents` from its positions, normals and first UV channel, splitting the
/// vertices whose triangles disagree on handedness.
pub(crate) fn generate(mesh: &mut ImportedMesh) {
    let vertex_count = mesh.positions.len();
    let no_uvs = vec![Vec2::zeros(); vertex_count];
    let uvs = mesh
        .uvs
        .first()
        .filter(|uvs| uvs.len() == vertex_count)
        .unwrap_or(&no_uvs);

    // The handedness of each triangle corner, and the frame sums of each (vertex, handedness).
    // Corners of triangles without UV area have none.
    let mut corner_signs: Vec<Option<i8>> = vec![None; mesh.indices.len()];
    let mut sums: HashMap<(u32, i8), Accumulated> = HashMap::new();
    for (triangle, signs) in mesh
        .indices
        .chunks_exact(3)
        .zip(corner_signs.chunks_exact_mut(3))
    {
        let [a, b, c] = [triangle[0], triangle[1], triangle[2]].map(|i| i as usize);
        let (p0, p1, p2) = (mesh.positions[a], mesh.positions[b], mesh.positions[c]);
        let (t0, t1, t2) = (uvs[a], uvs[b], uvs[c]);
        let (e1, e2) = (p1 - p0, p2 - p0);
        let (d1, d2) = (t1 - t0, t2 - t0);
        let det = d1.x * d2.y - d2.x * d1.y;
        if det.abs() < EPSILON {
            continue;
        }
        // Surface directions of increasing U and of decreasing V.
        let along_u = (e1 * d2.y - e2 * d1.y) / det;
        let up_v = -(e2 * d1.x - e1 * d2.x) / det;
        let (Some(along_u), Some(up_v)) = (normalized(along_u), normalized(up_v)) else {
            continue;
        };
        for (corner, &vertex) in triangle.iter().enumerate() {
            let normal = mesh.normals[vertex as usize];
            let sign = if normal.cross(&along_u).dot(&up_v) < 0.0 {
                -1
            } else {
                1
            };
            signs[corner] = Some(sign);
            let weight = corner_angle(
                mesh.positions[vertex as usize],
                mesh.positions[triangle[(corner + 1) % 3] as usize],
                mesh.positions[triangle[(corner + 2) % 3] as usize],
            );
            let sum = sums.entry((vertex, sign)).or_default();
            sum.tangent += project(along_u, normal) * weight;
            sum.bitangent += project(up_v, normal) * weight;
        }
    }

    // The vertex each (vertex, handedness) is stored in: the vertex itself for the first
    // handedness it is used with, a copy for the second. A corner without a handedness takes
    // its vertex's first.
    let mut first_signs: HashMap<u32, i8> = HashMap::new();
    for (&vertex, sign) in mesh.indices.iter().zip(&corner_signs) {
        if let Some(sign) = sign {
            first_signs.entry(vertex).or_insert(*sign);
        }
    }
    let mut targets: HashMap<(u32, i8), u32> = HashMap::new();
    let mut copies: Vec<u32> = Vec::new();
    for (index, sign) in mesh.indices.iter_mut().zip(&corner_signs) {
        let vertex = *index;
        let first_sign = first_signs.get(&vertex).copied().unwrap_or(1);
        let sign = sign.unwrap_or(first_sign);
        let is_copy = sign != first_sign;
        let next = (vertex_count + copies.len()) as u32;
        *index = *targets.entry((vertex, sign)).or_insert_with(|| {
            if is_copy {
                copies.push(vertex);
                next
            } else {
                vertex
            }
        });
    }
    split_vertices(mesh, &copies);

    let mut tangents = vec![None; mesh.positions.len()];
    for (&(vertex, sign), &target) in &targets {
        let normal = mesh.normals[target as usize];
        tangents[target as usize] = sums
            .get(&(vertex, sign))
            .and_then(|sum| normalized(project(sum.tangent, normal)))
            .map(|tangent| Vec4::new(tangent.x, tangent.y, tangent.z, f32::from(sign)));
    }
    mesh.tangents = tangents
        .into_iter()
        .zip(&mesh.normals)
        .map(|(tangent, normal)| tangent.unwrap_or_else(|| perpendicular(*normal)))
        .collect();
}

/// Appends a copy of each vertex in `copies`, with its attributes and skin weights.
fn split_vertices(mesh: &mut ImportedMesh, copies: &[u32]) {
    if copies.is_empty() {
        return;
    }
    let first_copy = mesh.positions.len() as u32;
    for &vertex in copies {
        let vertex = vertex as usize;
        mesh.positions.push(mesh.positions[vertex]);
        mesh.normals.push(mesh.normals[vertex]);
        for channel in &mut mesh.uvs {
            if let Some(&uv) = channel.get(vertex) {
                channel.push(uv);
            }
        }
    }
    let mut copies_of: HashMap<u32, Vec<u32>> = HashMap::new();
    for (offset, &vertex) in copies.iter().enumerate() {
        copies_of
            .entry(vertex)
            .or_default()
            .push(first_copy + offset as u32);
    }
    for bone in &mut mesh.bones {
        let added = bone
            .weights
            .iter()
            .flat_map(|&(vertex, weight)| {
                copies_of
                    .get(&vertex)
                    .into_iter()
                    .flatten()
                    .map(move |&copy| (copy, weight))
            })
            .collect::<Vec<_>>();
        bone.weights.extend(added);
    }
}

fn normalized(vector: Vec3) -> Option<Vec3> {
    let length = vector.norm();
    (length > EPSILON && length.is_finite()).then(|| vector / length)
}

/// `vector` without its component along the unit `normal`.
fn project(vector: Vec3, normal: Vec3) -> Vec3 {
    vector - normal * normal.dot(&vector)
}

/// Angle at `corner` of the triangle it makes with `next` and `previous`.
fn corner_angle(corner: Vec3, next: Vec3, previous: Vec3) -> f32 {
    match (normalized(next - corner), normalized(previous - corner)) {
        (Some(a), Some(b)) => a.dot(&b).clamp(-1.0, 1.0).acos(),
        _ => 0.0,
    }
}

/// A right-handed tangent perpendicular to `normal`, for a vertex no triangle gives a tangent.
fn perpendicular(normal: Vec3) -> Vec4 {
    let axis = if normal.x.abs() < 0.9 {
        Vec3::x()
    } else {
        Vec3::y()
    };
    let tangent = normalized(project(axis, normal)).unwrap_or_else(Vec3::x);
    Vec4::new(tangent.x, tangent.y, tangent.z, 1.0)
}

#[cfg(test)]
mod tests {
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
}
