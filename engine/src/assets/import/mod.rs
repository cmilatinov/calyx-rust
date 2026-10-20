//! Model import: turns a model file into an [`ImportedScene`], the library-independent form the
//! mesh, prefab and animation assets are built from.

mod gltf;
mod ufbx;

use std::collections::HashMap;
use std::path::Path;

use nalgebra_glm::{Mat4, Quat, Vec2, Vec3};

use crate::assets::animation::AnimationKeyFrames;
use crate::assets::error::AssetError;

/// Number of UV channels an imported mesh carries.
pub(crate) const MAX_UV_CHANNELS: usize = 4;

/// A model file's meshes, node tree and animations.
pub(crate) struct ImportedScene {
    pub meshes: Vec<ImportedMesh>,
    pub root: ImportedNode,
    pub animations: Vec<ImportedAnimation>,
}

/// One triangle mesh, split per material, with its skin.
pub(crate) struct ImportedMesh {
    pub name: String,
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    /// UV channels in use, at most [`MAX_UV_CHANNELS`].
    pub uvs: Vec<Vec<Vec2>>,
    /// Triangle list, three indices per triangle, in the winding the renderer expects.
    pub indices: Vec<u32>,
    pub bones: Vec<ImportedBone>,
}

/// A bone influencing an [`ImportedMesh`].
pub(crate) struct ImportedBone {
    /// Name of the node the bone follows.
    pub name: String,
    /// Transform from mesh space to bone space in the bind pose.
    pub inverse_bind_transform: Mat4,
    /// Influenced vertices and their weights.
    pub weights: Vec<(u32, f32)>,
}

/// A node of the imported tree.
pub(crate) struct ImportedNode {
    pub name: String,
    /// Transform relative to the parent node.
    pub transform: Mat4,
    /// Indices into [`ImportedScene::meshes`].
    pub meshes: Vec<usize>,
    pub children: Vec<ImportedNode>,
}

/// An animation clip with key times in seconds.
pub(crate) struct ImportedAnimation {
    pub name: String,
    pub duration_seconds: f64,
    /// Keyframes by node name.
    pub channels: HashMap<String, AnimationKeyFrames>,
}

/// Imports the model file at `path`.
pub(crate) fn import(path: &Path) -> Result<ImportedScene, AssetError> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("gltf" | "glb") => gltf::import(path),
        _ => ufbx::import(path),
    }
    .map_err(|error| error.with_path(path))
}

/// Builds an [`ImportedMesh`] from triangle corners, merging corners whose attributes are
/// identical, as assimp's `JoinIdenticalVertices` does.
struct MeshBuilder {
    mesh: ImportedMesh,
    corners: HashMap<CornerKey, u32>,
    /// Source control point of each output vertex, for skin weights.
    control_points: Vec<u32>,
}

#[derive(PartialEq, Eq, Hash)]
struct CornerKey {
    skin_weights: u32,
    position: [u32; 3],
    normal: [u32; 3],
    uvs: [[u32; 2]; MAX_UV_CHANNELS],
}

/// One triangle corner before merging.
struct Corner {
    /// Source control point, which carries the skin weights.
    control_point: u32,
    /// Identifies the corner's set of skin weights: corners merge only when theirs are equal.
    skin_weights: u32,
    position: Vec3,
    normal: Vec3,
    /// UVs with the origin at the top left, as the renderer samples them.
    uvs: [Vec2; MAX_UV_CHANNELS],
}

impl MeshBuilder {
    fn new(name: String, uv_channels: usize) -> Self {
        Self {
            mesh: ImportedMesh {
                name,
                positions: Vec::new(),
                normals: Vec::new(),
                uvs: vec![Vec::new(); uv_channels.min(MAX_UV_CHANNELS)],
                indices: Vec::new(),
                bones: Vec::new(),
            },
            corners: HashMap::new(),
            control_points: Vec::new(),
        }
    }

    /// Adds a triangle given in the source winding; it is stored reversed, as assimp's
    /// `FlipWindingOrder` does.
    fn push_triangle(&mut self, corners: [Corner; 3]) {
        let [a, b, c] = corners.map(|corner| self.vertex(corner));
        self.mesh.indices.extend([a, c, b]);
    }

    fn vertex(&mut self, corner: Corner) -> u32 {
        let uvs = corner.uvs;
        let key = CornerKey {
            skin_weights: corner.skin_weights,
            position: corner.position.map(value_bits).into(),
            normal: corner.normal.map(value_bits).into(),
            uvs: uvs.map(|uv| uv.map(value_bits).into()),
        };
        let next = self.mesh.positions.len() as u32;
        *self.corners.entry(key).or_insert_with(|| {
            self.mesh.positions.push(corner.position);
            self.mesh.normals.push(corner.normal);
            for (channel, uv) in self.mesh.uvs.iter_mut().zip(uvs) {
                channel.push(uv);
            }
            self.control_points.push(corner.control_point);
            next
        })
    }

    fn finish(self) -> (ImportedMesh, Vec<u32>) {
        (self.mesh, self.control_points)
    }
}

/// Bits of `value` with negative zero read as zero, so corners compare by value as assimp's
/// `JoinIdenticalVertices` compares them.
fn value_bits(value: f32) -> u32 {
    (value + 0.0).to_bits()
}

/// Smooth vertex normals for triangles that carry none, as assimp's `GenerateSmoothNormals`
/// computes them: each corner gets the normalised sum of the unit face normals of every corner
/// lying within 1e-4 of the bounding box diagonal of it, so seams whose copies of a position
/// differ by rounding are smoothed across. `triangles` are in the source winding. A polygon
/// counts once per triangle it was split into, where assimp counts it once, so non-planar
/// polygons can differ slightly.
fn smooth_normals(triangles: &[[Vec3; 3]]) -> Vec<[Vec3; 3]> {
    let corners = triangles.iter().flatten();
    let (min, max) = corners.clone().fold(
        (Vec3::repeat(f32::INFINITY), Vec3::repeat(f32::NEG_INFINITY)),
        |(min, max), p| (min.inf(p), max.sup(p)),
    );
    let epsilon = (max - min).norm() * 1e-4;
    let faces = triangles
        .iter()
        .map(|[a, b, c]| {
            let face = (b - a).cross(&(c - a));
            let length = face.norm();
            if length > 0.0 {
                face / length
            } else {
                Vec3::zeros()
            }
        })
        .collect::<Vec<_>>();

    // Bucket corners in a grid of `epsilon` cells, so neighbours are found in adjacent cells.
    let cell = |p: &Vec3| -> [i64; 3] {
        if epsilon > 0.0 {
            p.map(|value| (value / epsilon).floor() as i64).into()
        } else {
            [0; 3]
        }
    };
    let mut grid: HashMap<[i64; 3], Vec<(Vec3, usize)>> = HashMap::new();
    for (face, triangle) in triangles.iter().enumerate() {
        for p in triangle {
            grid.entry(cell(p)).or_default().push((*p, face));
        }
    }
    let smoothed = |p: &Vec3| {
        let [x, y, z] = cell(p);
        let mut sum = Vec3::zeros();
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    for (q, face) in grid.get(&[x + dx, y + dy, z + dz]).into_iter().flatten() {
                        if (q - p).norm_squared() < epsilon * epsilon || q == p {
                            sum += faces[*face];
                        }
                    }
                }
            }
        }
        let length = sum.norm();
        if length > 0.0 {
            sum / length
        } else {
            sum
        }
    };
    triangles
        .iter()
        .map(|triangle| triangle.map(|p| smoothed(&p)))
        .collect()
}

/// Splits a quad into two triangles as assimp's `Triangulate` does: from the first concave
/// corner, found by the angles it makes with its neighbours and the opposite corner, or from
/// corner 0 when the quad is convex. Returns positions into `quad`.
fn split_quad(quad: [Vec3; 4]) -> [[usize; 3]; 2] {
    let start = (0..4)
        .find(|&i| {
            let corner = quad[i];
            let left = (quad[(i + 3) % 4] - corner).normalize();
            let diagonal = (quad[(i + 2) % 4] - corner).normalize();
            let right = (quad[(i + 1) % 4] - corner).normalize();
            left.dot(&diagonal).acos() + right.dot(&diagonal).acos() > std::f32::consts::PI
        })
        .unwrap_or(0);
    let at = |offset: usize| (start + offset) % 4;
    [[at(0), at(1), at(2)], [at(0), at(2), at(3)]]
}

/// Splits a polygon of five or more corners into triangles as assimp's `Triangulate` does: the
/// polygon is projected onto the plane its Newell normal is closest to, then ears are cut in
/// sequence, each search starting after the corner that closed the previous ear. Falls back to
/// a fan when no ear is found, as assimp does for polygons that are not simple. Returns
/// positions into `polygon`.
fn clip_ears(polygon: &[Vec3]) -> Vec<[usize; 3]> {
    let count = polygon.len();
    let mut normal = Vec3::zeros();
    for (a, b) in polygon.iter().zip(polygon.iter().cycle().skip(1)) {
        normal.x += (a.y - b.y) * (a.z + b.z);
        normal.y += (a.z - b.z) * (a.x + b.x);
        normal.z += (a.x - b.x) * (a.y + b.y);
    }
    let (mut u, mut v, mut sign) = (0, 1, normal.z);
    if normal.x.abs() > normal.y.abs() {
        if normal.x.abs() > normal.z.abs() {
            (u, v, sign) = (1, 2, normal.x);
        }
    } else if normal.y.abs() > normal.z.abs() {
        (u, v, sign) = (2, 0, normal.y);
    }
    if sign < 0.0 {
        std::mem::swap(&mut u, &mut v);
    }
    let points = polygon
        .iter()
        .map(|p| [p[u] as f64, p[v] as f64])
        .collect::<Vec<_>>();
    let area = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        0.5 * (a[0] * (c[1] - b[1]) + b[0] * (a[1] - c[1]) + c[0] * (b[1] - a[1]))
    };
    let inside = |p0: [f64; 2], p1: [f64; 2], p2: [f64; 2], p: [f64; 2]| {
        let (e0, e1, e2) = (
            [p1[0] - p0[0], p1[1] - p0[1]],
            [p2[0] - p0[0], p2[1] - p0[1]],
            [p[0] - p0[0], p[1] - p0[1]],
        );
        let dot = |a: [f64; 2], b: [f64; 2]| a[0] * b[0] + a[1] * b[1];
        let (d00, d01, d02, d11, d12) = (
            dot(e0, e0),
            dot(e0, e1),
            dot(e0, e2),
            dot(e1, e1),
            dot(e1, e2),
        );
        let inverse = 1.0 / (d00 * d11 - d01 * d01);
        let s = (d11 * d02 - d01 * d12) * inverse;
        let t = (d00 * d12 - d01 * d02) * inverse;
        s > 0.0 && t > 0.0 && s + t < 1.0
    };

    let mut clipped = vec![false; count];
    let after = |i: usize, clipped: &[bool]| {
        let mut j = (i + 1) % count;
        while clipped[j] {
            j = (j + 1) % count;
        }
        j
    };
    let mut triangles = Vec::with_capacity(count - 2);
    let mut prev = count - 2;
    for remaining in (3..=count).rev() {
        let mut ear = after(prev, &clipped);
        let mut next = after(ear, &clipped);
        let mut attempts = 0;
        loop {
            let (p0, p1, p2) = (points[prev], points[ear], points[next]);
            let is_ear = area(p1, p0, p2) > 0.0
                && points
                    .iter()
                    .all(|&p| p == p0 || p == p1 || p == p2 || !inside(p0, p1, p2, p));
            if is_ear {
                break;
            }
            attempts += 1;
            if attempts > 2 * remaining {
                // Not a simple polygon: fan the corners that are left.
                let left = (0..count).filter(|&i| !clipped[i]).collect::<Vec<_>>();
                triangles.extend(left.windows(2).skip(1).map(|w| [left[0], w[0], w[1]]));
                return triangles;
            }
            (prev, ear, next) = (ear, next, after(next, &clipped));
        }
        triangles.push([prev, ear, next]);
        clipped[ear] = true;
        prev = next;
    }
    triangles
}

/// Converts a rotation given as `(x, y, z, w)`.
fn quat(x: f64, y: f64, z: f64, w: f64) -> Quat {
    Quat::new(w as f32, x as f32, y as f32, z as f32)
}
