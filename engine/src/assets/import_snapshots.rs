//! Import snapshot tests: each model file is imported through its asset type and summarised into
//! order-independent metrics, then compared against a committed reference snapshot within a float
//! tolerance. The references for OBJ and FBX files were captured from the assimp importer, so
//! these tests check that later importers produce the same meshes, hierarchy, skinning and
//! animation. The GLB fixture's reference was captured from the glTF importer once its animated
//! poses were shown to match the FBX reference of the same model.
//!
//! Set `CALYX_UPDATE_IMPORT_SNAPSHOTS=1` to rewrite the references from the current importer.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use nalgebra_glm::{IVec4, Mat4, Vec2, Vec3, Vec4};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::assets::animation::Animation;
use crate::assets::mesh::Mesh;
use crate::assets::Asset;
use crate::component::{
    ComponentAnimator, ComponentBone, ComponentMesh, ComponentSkinnedMesh, ComponentTransform,
};
use crate::context::ReadOnlyAssetContext;
use crate::math::Transform;
use crate::scene::Prefab;
use crate::test_support::TestAssets;
use crate::utils::TypeUuid;

/// Absolute and relative tolerance of a snapshot comparison.
const TOLERANCE: f64 = 1e-3;
/// Tolerance of comparing two formats of one model, whose keys were resampled by the converter.
const CROSS_FORMAT_TOLERANCE: f64 = 5e-3;
/// Length units per metre in the FBX fixture, which Blender writes in centimetres.
const FBX_FIXTURE_UNITS_PER_METRE: f64 = 100.0;
const ANIMATION_SAMPLES: usize = 10;

/// Order-independent metrics of a triangle soup. Every metric but `size` is divided by the
/// power of `size` that makes it unitless, so the comparison tolerance is relative to the size of
/// the model.
#[derive(Serialize)]
struct Shape {
    triangles: usize,
    /// Diagonal of the bounding box.
    size: f64,
    bounds_min: [f64; 3],
    bounds_max: [f64; 3],
    area: f64,
    /// Area-weighted centroid.
    centroid: [f64; 3],
    /// Second moments of the surface, as area integrals: xx, yy, zz, xy, xz, yz.
    second_moments: [f64; 6],
    /// Signed volume enclosed by the triangles; its sign follows the winding.
    signed_volume: f64,
}

impl Shape {
    fn new(positions: &[Vec3], indices: &[u32]) -> Self {
        let mut bounds_min = [f64::INFINITY; 3];
        let mut bounds_max = [f64::NEG_INFINITY; 3];
        let mut area = 0.0;
        let mut centroid = [0.0; 3];
        let mut second_moments = [0.0; 6];
        let mut signed_volume = 0.0;
        for triangle in indices.chunks_exact(3) {
            let [a, b, c] = [0, 1, 2].map(|i| to_f64(&positions[triangle[i] as usize]));
            for p in [a, b, c] {
                for axis in 0..3 {
                    bounds_min[axis] = bounds_min[axis].min(p[axis]);
                    bounds_max[axis] = bounds_max[axis].max(p[axis]);
                }
            }
            let triangle_area = norm(cross(sub(b, a), sub(c, a))) / 2.0;
            let center = [0, 1, 2].map(|axis| (a[axis] + b[axis] + c[axis]) / 3.0);
            area += triangle_area;
            for axis in 0..3 {
                centroid[axis] += triangle_area * center[axis];
            }
            // Exact area integral of x_i * x_j over the triangle, so any triangulation of a flat
            // polygon gives the same sum.
            let sum = [0, 1, 2].map(|axis| a[axis] + b[axis] + c[axis]);
            for (moment, (i, j)) in
                second_moments
                    .iter_mut()
                    .zip([(0, 0), (1, 1), (2, 2), (0, 1), (0, 2), (1, 2)])
            {
                let corners = a[i] * a[j] + b[i] * b[j] + c[i] * c[j];
                *moment += triangle_area / 12.0 * (corners + sum[i] * sum[j]);
            }
            signed_volume += dot(a, cross(b, c)) / 6.0;
        }
        if area > 0.0 {
            centroid = centroid.map(|value| value / area);
        }
        let size = norm(sub(bounds_max, bounds_min));
        let unitless = |value: f64, power: i32| {
            if size > 0.0 {
                value / size.powi(power)
            } else {
                value
            }
        };
        Self {
            triangles: indices.len() / 3,
            size,
            bounds_min: bounds_min.map(|value| unitless(value, 1)),
            bounds_max: bounds_max.map(|value| unitless(value, 1)),
            area: unitless(area, 2),
            centroid: centroid.map(|value| unitless(value, 1)),
            second_moments: second_moments.map(|value| unitless(value, 4)),
            signed_volume: unitless(signed_volume, 3),
        }
    }
}

/// Metrics of a mesh asset in its own space.
#[derive(Serialize)]
struct MeshSummary {
    shape: Shape,
    /// Mean over the distinct positions of the mean normal at each position, which does not
    /// depend on how polygons were split, how vertices were merged, or rounding differences
    /// between copies of a normal.
    mean_normal: [f64; 3],
    /// Fraction of the surface area whose corner normals point out of the front face given by
    /// the winding, minus the fraction where they point out of the back: 1 or -1 when normals and
    /// winding agree everywhere.
    normal_alignment: f64,
    /// Per UV channel in use: signed UV area, whose sign follows a UV flip, and area-weighted mean
    /// UV.
    uv_channels: Vec<UvSummary>,
    skin: Option<SkinSummary>,
}

#[derive(Serialize)]
struct UvSummary {
    signed_area: f64,
    mean: [f64; 2],
}

#[derive(Serialize)]
struct SkinSummary {
    /// Number of vertices with at least one bone influence.
    skinned_vertices: usize,
    /// Sum of all bone weights.
    total_weight: f64,
    /// Inverse bind matrix of each bone, by bone name.
    inverse_bind_matrices: BTreeMap<String, [f64; 16]>,
}

impl MeshSummary {
    fn new(mesh: &Mesh) -> Self {
        let mut position_normals: HashMap<[u32; 3], HashMap<[u32; 3], [f64; 3]>> = HashMap::new();
        let mut normal_alignment = 0.0;
        let mut total_area = 0.0;
        let channels = mesh
            .uvs
            .iter()
            .filter(|uvs| uvs.iter().any(|uv| *uv != Vec2::zeros()))
            .collect::<Vec<_>>();
        let mut uv_channels = channels
            .iter()
            .map(|_| UvSummary {
                signed_area: 0.0,
                mean: [0.0; 2],
            })
            .collect::<Vec<_>>();
        for triangle in mesh.indices.chunks_exact(3) {
            let [a, b, c] = [0, 1, 2].map(|i| to_f64(&mesh.vertices[triangle[i] as usize]));
            let face = cross(sub(b, a), sub(c, a));
            let area = norm(face) / 2.0;
            if area == 0.0 {
                continue;
            }
            total_area += area;
            let corner_normals = triangle
                .iter()
                .map(|&index| to_f64(&mesh.normals[index as usize]))
                .fold([0.0; 3], |sum, normal| {
                    [0, 1, 2].map(|axis| sum[axis] + normal[axis])
                });
            normal_alignment += area * dot(corner_normals, face).signum();
            for &index in triangle {
                let vertex = &mesh.vertices[index as usize];
                let normal = &mesh.normals[index as usize];
                position_normals
                    .entry([vertex.x, vertex.y, vertex.z].map(f32::to_bits))
                    .or_default()
                    .insert(
                        [normal.x, normal.y, normal.z].map(f32::to_bits),
                        to_f64(normal),
                    );
            }
            for (summary, uvs) in uv_channels.iter_mut().zip(&channels) {
                let [ua, ub, uc] = [0, 1, 2].map(|i| uvs[triangle[i] as usize]);
                let (e1, e2) = (ub - ua, uc - ua);
                summary.signed_area +=
                    (e1.x as f64 * e2.y as f64 - e1.y as f64 * e2.x as f64) / 2.0;
                for uv in [ua, ub, uc] {
                    summary.mean[0] += area * uv.x as f64 / 3.0;
                    summary.mean[1] += area * uv.y as f64 / 3.0;
                }
            }
        }
        let mut mean_normal = [0.0; 3];
        for normals in position_normals.values() {
            for normal in normals.values() {
                for axis in 0..3 {
                    mean_normal[axis] += normal[axis] / normals.len() as f64;
                }
            }
        }
        if !position_normals.is_empty() {
            mean_normal = mean_normal.map(|value| value / position_normals.len() as f64);
        }
        if total_area > 0.0 {
            normal_alignment /= total_area;
            for summary in &mut uv_channels {
                summary.mean = summary.mean.map(|value| value / total_area);
            }
        }
        let skin = (!mesh.bones.is_empty()).then(|| SkinSummary {
            skinned_vertices: mesh
                .bone_indices
                .iter()
                .filter(|indices| indices.iter().any(|index| *index >= 0))
                .count(),
            total_weight: mesh
                .bone_indices
                .iter()
                .zip(&mesh.bone_weights)
                .map(|(indices, weights)| bound_weight(indices, weights))
                .sum(),
            inverse_bind_matrices: mesh
                .bones
                .iter()
                .map(|(name, bone)| (name.clone(), matrix(&bone.inverse_bind_transform)))
                .collect(),
        });
        Self {
            shape: Shape::new(&mesh.vertices, &mesh.indices),
            mean_normal,
            normal_alignment,
            uv_channels,
            skin,
        }
    }
}

/// Metrics of an imported prefab: its mesh instances and skeleton in the space of the prefab
/// root, and the skinned shapes at sampled animation times.
#[derive(Serialize)]
struct PrefabSummary {
    /// Mesh instances, sorted by triangle count and root-space centroid.
    meshes: Vec<MeshInstanceSummary>,
    /// Root-space bind transform of each bone, by bone name.
    bones: BTreeMap<String, [f64; 16]>,
    animations: Vec<AnimationSummary>,
}

#[derive(Serialize)]
struct MeshInstanceSummary {
    skinned: bool,
    /// Transform of the mesh object relative to the prefab root.
    root_transform: [f64; 16],
    mesh: MeshSummary,
    /// Static meshes: the mesh placed by `root_transform`. Skinned meshes: the mesh skinned by
    /// the bind pose.
    root_shape: Shape,
}

#[derive(Serialize)]
struct AnimationSummary {
    duration_seconds: f64,
    /// Samples at evenly spaced times over the clip.
    samples: Vec<AnimationSample>,
}

#[derive(Serialize)]
struct AnimationSample {
    time_seconds: f64,
    /// Root-space transform of each bone, by bone name.
    bones: BTreeMap<String, [f64; 16]>,
    /// Shape of each skinned mesh instance, in `meshes` order.
    skinned_shapes: Vec<Shape>,
}

/// Prefab hierarchy read back from its scene data.
struct PrefabNodes {
    root: Uuid,
    children: HashMap<Uuid, Vec<Uuid>>,
    transforms: HashMap<Uuid, Transform>,
    /// Bone name of each bone object.
    bones: HashMap<Uuid, String>,
    meshes: Vec<(Uuid, Uuid, bool)>,
}

impl PrefabNodes {
    fn new(prefab: &Prefab) -> Self {
        let mut transforms = HashMap::new();
        let mut bones = HashMap::new();
        let mut meshes = Vec::new();
        for (id, components) in &prefab.data.components {
            if let Some(value) = components.get(&ComponentTransform::type_uuid()) {
                transforms.insert(
                    *id,
                    serde_json::from_value(value["transform"].clone()).unwrap(),
                );
            }
            if let Some(value) = components.get(&ComponentBone::type_uuid()) {
                bones.insert(*id, value["name"].as_str().unwrap().to_string());
            }
            for (type_uuid, skinned) in [
                (ComponentMesh::type_uuid(), false),
                (ComponentSkinnedMesh::type_uuid(), true),
            ] {
                if let Some(value) = components.get(&type_uuid) {
                    let mesh = value["mesh"].as_str().unwrap().parse().unwrap();
                    meshes.push((*id, mesh, skinned));
                }
            }
        }
        Self {
            root: prefab.root,
            children: prefab.data.hierarchy.clone(),
            transforms,
            bones,
            meshes,
        }
    }

    /// Visits every node below the root with its transform relative to the root, using
    /// `local` for each node's local transform, as the animator does: the root's own
    /// transform is cancelled out.
    fn walk(&self, local: &impl Fn(Uuid) -> Mat4, visit: &mut impl FnMut(Uuid, &Mat4)) {
        fn recurse(
            nodes: &PrefabNodes,
            id: Uuid,
            parent: Mat4,
            local: &impl Fn(Uuid) -> Mat4,
            visit: &mut impl FnMut(Uuid, &Mat4),
        ) {
            let transform = parent * local(id);
            visit(id, &transform);
            for child in nodes.children.get(&id).into_iter().flatten() {
                recurse(nodes, *child, transform, local, visit);
            }
        }
        for child in self.children.get(&self.root).into_iter().flatten() {
            recurse(self, *child, Mat4::identity(), local, visit);
        }
        visit(self.root, &Mat4::identity());
    }

    fn static_local(&self, id: Uuid) -> Mat4 {
        self.transforms
            .get(&id)
            .map(Transform::matrix)
            .unwrap_or_else(Mat4::identity)
    }

    /// Root-relative node transforms, and those of the bones by bone name.
    fn pose(&self, local: &impl Fn(Uuid) -> Mat4) -> Pose {
        let mut pose = Pose {
            world: HashMap::new(),
            bones: HashMap::new(),
        };
        self.walk(local, &mut |id, transform| {
            pose.world.insert(id, *transform);
            if let Some(name) = self.bones.get(&id) {
                pose.bones.insert(name.clone(), *transform);
            }
        });
        pose
    }

    fn bone_transforms(&self, world: &HashMap<Uuid, Mat4>) -> BTreeMap<String, [f64; 16]> {
        self.bones
            .iter()
            .map(|(id, name)| (name.clone(), matrix(&world[id])))
            .collect()
    }
}

/// Root-relative transforms of a posed prefab.
struct Pose {
    world: HashMap<Uuid, Mat4>,
    bones: HashMap<String, Mat4>,
}

impl Pose {
    /// The skinning matrices of `mesh` by its own bone indices, as the animator builds them: each
    /// of the mesh's bones follows the bone object of the same name.
    fn skinning(&self, mesh: &Mesh) -> Vec<Mat4> {
        let mut skinning = vec![Mat4::identity(); mesh.bones.len()];
        for (name, bone) in &mesh.bones {
            if let (Some(transform), Some(slot)) =
                (self.bones.get(name), skinning.get_mut(bone.index))
            {
                *slot = transform * bone.inverse_bind_transform;
            }
        }
        skinning
    }
}

fn skinned_positions(mesh: &Mesh, skinning: &[Mat4]) -> Vec<Vec3> {
    mesh.vertices
        .iter()
        .zip(mesh.bone_indices.iter().zip(&mesh.bone_weights))
        .map(|(vertex, (indices, weights))| {
            let point = Vec4::new(vertex.x, vertex.y, vertex.z, 1.0);
            let mut skinned = Vec4::zeros();
            for (index, weight) in indices.iter().zip(weights.iter()) {
                if *index >= 0 {
                    let bone = skinning
                        .get(*index as usize)
                        .copied()
                        .unwrap_or_else(Mat4::identity);
                    skinned += *weight * (bone * point);
                }
            }
            skinned.xyz()
        })
        .collect()
}

fn placed_positions(mesh: &Mesh, transform: &Mat4) -> Vec<Vec3> {
    mesh.vertices
        .iter()
        .map(|vertex| (transform * Vec4::new(vertex.x, vertex.y, vertex.z, 1.0)).xyz())
        .collect()
}

impl PrefabSummary {
    fn new(context: &ReadOnlyAssetContext, prefab: &Prefab, sub_assets: &[Uuid]) -> Self {
        let registry = context.registries.assets.read();
        let nodes = PrefabNodes::new(prefab);
        let static_local = |id| nodes.static_local(id);
        let bind = nodes.pose(&static_local);

        let mut instances = nodes
            .meshes
            .iter()
            .map(|(id, mesh_id, skinned)| {
                let mesh = registry.loaded_by_id::<Mesh>(*mesh_id).unwrap();
                let mesh_ref = mesh.read();
                let root_shape = if *skinned {
                    Shape::new(
                        &skinned_positions(&mesh_ref, &bind.skinning(&mesh_ref)),
                        &mesh_ref.indices,
                    )
                } else {
                    Shape::new(
                        &placed_positions(&mesh_ref, &bind.world[id]),
                        &mesh_ref.indices,
                    )
                };
                let summary = MeshInstanceSummary {
                    skinned: *skinned,
                    root_transform: matrix(&bind.world[id]),
                    mesh: MeshSummary::new(&mesh_ref),
                    root_shape,
                };
                (summary, mesh.clone())
            })
            .collect::<Vec<_>>();
        instances.sort_by_key(|(summary, _)| {
            (
                summary.mesh.shape.triangles,
                summary
                    .root_shape
                    .centroid
                    .map(|value| (value * 100.0).round() as i64),
            )
        });

        let animations = sub_assets
            .iter()
            .filter_map(|id| registry.loaded_by_id::<Animation>(*id))
            .map(|animation| {
                let animation = animation.read();
                let duration_seconds = animation.duration / animation.ticks_per_second;
                let samples = (0..ANIMATION_SAMPLES)
                    .map(|sample| {
                        let time_seconds =
                            duration_seconds * sample as f64 / ANIMATION_SAMPLES as f64;
                        let ticks = ((time_seconds * animation.ticks_per_second) as f32)
                            % animation.duration as f32;
                        let local = |id: Uuid| {
                            let Some(name) = nodes.bones.get(&id) else {
                                return nodes.static_local(id);
                            };
                            let Some(keyframes) = animation.node_keyframes.get(name) else {
                                return nodes.static_local(id);
                            };
                            let bind_pose = nodes.transforms.get(&id).copied().unwrap_or_default();
                            ComponentAnimator::animation_local_bone_transform(
                                keyframes, ticks, &bind_pose,
                            )
                            .matrix()
                        };
                        let pose = nodes.pose(&local);
                        AnimationSample {
                            time_seconds,
                            bones: nodes.bone_transforms(&pose.world),
                            skinned_shapes: instances
                                .iter()
                                .filter(|(summary, _)| summary.skinned)
                                .map(|(_, mesh)| {
                                    let mesh = mesh.read();
                                    Shape::new(
                                        &skinned_positions(&mesh, &pose.skinning(&mesh)),
                                        &mesh.indices,
                                    )
                                })
                                .collect(),
                        }
                    })
                    .collect();
                AnimationSummary {
                    duration_seconds,
                    samples,
                }
            })
            .collect::<Vec<_>>();

        Self {
            meshes: instances.into_iter().map(|(summary, _)| summary).collect(),
            bones: nodes.bone_transforms(&bind.world),
            animations,
        }
    }
}

fn bound_weight(indices: &IVec4, weights: &Vec4) -> f64 {
    indices
        .iter()
        .zip(weights.iter())
        .filter(|(index, _)| **index >= 0)
        .map(|(_, weight)| *weight as f64)
        .sum()
}

fn to_f64(v: &Vec3) -> [f64; 3] {
    [v.x as f64, v.y as f64, v.z as f64]
}

fn matrix(m: &Mat4) -> [f64; 16] {
    std::array::from_fn(|i| m.as_slice()[i] as f64)
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

fn snapshot_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/import_snapshots")
        .join(format!("{name}.json"))
}

/// Compares `actual` with the reference snapshot `name`, or rewrites the reference when
/// `CALYX_UPDATE_IMPORT_SNAPSHOTS` is set.
fn check_snapshot(name: &str, actual: &impl Serialize) {
    let actual = serde_json::to_value(actual).unwrap();
    let path = snapshot_path(name);
    if std::env::var_os("CALYX_UPDATE_IMPORT_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut text = serde_json::to_string_pretty(&actual).unwrap();
        text.push('\n');
        std::fs::write(&path, text).unwrap();
        return;
    }
    assert_matches(name, &read_snapshot(name), &actual, TOLERANCE);
}

fn read_snapshot(name: &str) -> Value {
    let path = snapshot_path(name);
    serde_json::from_str(
        &std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("missing snapshot {}: {e}", path.display())),
    )
    .unwrap()
}

fn assert_matches(name: &str, expected: &Value, actual: &Value, tolerance: f64) {
    let mut mismatches = Vec::new();
    compare(
        expected,
        actual,
        name.to_string(),
        tolerance,
        &mut mismatches,
    );
    assert!(
        mismatches.is_empty(),
        "import snapshot {name} differs:\n{}",
        mismatches.join("\n")
    );
}

/// Compares numbers within `tolerance`, absolute plus relative.
fn compare(
    expected: &Value,
    actual: &Value,
    path: String,
    tolerance: f64,
    mismatches: &mut Vec<String>,
) {
    match (expected, actual) {
        (Value::Number(e), Value::Number(a)) => {
            let (e, a) = (e.as_f64().unwrap(), a.as_f64().unwrap());
            if (e - a).abs() > tolerance * (1.0 + e.abs().max(a.abs())) {
                mismatches.push(format!("{path}: expected {e}, got {a}"));
            }
        }
        (Value::Array(e), Value::Array(a)) if e.len() == a.len() => {
            for (i, (e, a)) in e.iter().zip(a).enumerate() {
                compare(e, a, format!("{path}[{i}]"), tolerance, mismatches);
            }
        }
        (Value::Object(e), Value::Object(a)) if e.keys().eq(a.keys()) => {
            for (key, e) in e {
                compare(e, &a[key], format!("{path}.{key}"), tolerance, mismatches);
            }
        }
        _ if expected == actual => {}
        _ => mismatches.push(format!("{path}: expected {expected}, got {actual}")),
    }
}

fn mesh_snapshot(root: impl Fn(&TestAssets) -> &Path, relative_path: &str, name: &str) {
    let assets = TestAssets::models();
    let context = assets.asset_context();
    let context = context.lock_read();
    let path = root(&assets).join(relative_path);
    let mesh = Mesh::from_file(&context, &path)
        .unwrap_or_else(|e| panic!("failed to import {relative_path}: {e}"))
        .asset;
    check_snapshot(name, &MeshSummary::new(&mesh));
}

fn prefab_summary(root: impl Fn(&TestAssets) -> &Path, relative_path: &str) -> Value {
    let assets = TestAssets::models();
    let context = assets.asset_context();
    let context = context.lock_read();
    let path = root(&assets).join(relative_path);
    let loaded = Prefab::from_file(&context, &path)
        .unwrap_or_else(|e| panic!("failed to import {relative_path}: {e}"));
    serde_json::to_value(PrefabSummary::new(
        &context,
        &loaded.asset,
        &loaded.sub_assets,
    ))
    .unwrap()
}

fn prefab_snapshot(root: impl Fn(&TestAssets) -> &Path, relative_path: &str, name: &str) {
    check_snapshot(name, &prefab_summary(root, relative_path));
}

/// The animation samples of a prefab summary, with shape sizes dropped and bone transforms
/// divided by `units_per_metre`, so two formats of one model compare whatever their units.
fn animated_poses(summary: &Value, units_per_metre: f64) -> Value {
    let mut animations = summary["animations"].clone();
    for animation in animations.as_array_mut().unwrap() {
        for sample in animation["samples"].as_array_mut().unwrap() {
            for shape in sample["skinned_shapes"].as_array_mut().unwrap() {
                shape.as_object_mut().unwrap().remove("size");
            }
            for bone in sample["bones"].as_object_mut().unwrap().values_mut() {
                // Every element but the homogeneous 1 scales with the unit.
                for value in &mut bone.as_array_mut().unwrap()[..15] {
                    *value = (value.as_f64().unwrap() / units_per_metre).into();
                }
            }
        }
    }
    animations
}

#[test]
fn cube_obj_matches_snapshot() {
    mesh_snapshot(TestAssets::engine_root, "meshes/cube.obj", "cube_obj");
}

#[test]
fn teapot_obj_matches_snapshot() {
    mesh_snapshot(TestAssets::engine_root, "meshes/teapot.obj", "teapot_obj");
}

#[test]
fn tank_turret_head_obj_matches_snapshot() {
    mesh_snapshot(
        TestAssets::sandbox_root,
        "meshes/tank_game/turret_head.obj",
        "tank_turret_head_obj",
    );
}

#[test]
fn tank_fbx_matches_snapshot() {
    prefab_snapshot(
        TestAssets::sandbox_root,
        "models/tank_game/Tank_002.fbx",
        "tank_fbx",
    );
}

#[test]
fn rigged_figure_fbx_matches_snapshot() {
    prefab_snapshot(
        TestAssets::fixtures_root,
        "fbx/RiggedFigure.fbx",
        "rigged_figure",
    );
}

#[test]
fn rigged_figure_starting_at_a_later_frame_plays_like_the_original() {
    // The same clip with its keys moved to frames 30-60 must play from its first key.
    prefab_snapshot(
        TestAssets::fixtures_root,
        "fbx/RiggedFigureOffset.fbx",
        "rigged_figure",
    );
}

/// The fixture GLB plays like the FBX that Blender converted from it, whose reference was
/// captured from assimp: the skinned shapes and root-space bone transforms match at every sampled
/// time. The static pose is not compared, because Blender writes the FBX nodes at the clip's first
/// frame rather than in the glTF rest pose.
#[test]
fn rigged_figure_glb_plays_like_the_fbx_reference() {
    let actual = prefab_summary(TestAssets::fixtures_root, "glb/RiggedFigure.glb");
    assert_matches(
        "rigged_figure",
        &animated_poses(&read_snapshot("rigged_figure"), FBX_FIXTURE_UNITS_PER_METRE),
        &animated_poses(&actual, 1.0),
        CROSS_FORMAT_TOLERANCE,
    );
}

#[test]
fn rigged_figure_glb_matches_snapshot() {
    prefab_snapshot(
        TestAssets::fixtures_root,
        "glb/RiggedFigure.glb",
        "rigged_figure_glb",
    );
}

/// Two meshes skinned by one joint tree, whose skins list the joints in different orders and
/// counts: each mesh follows its own bones.
#[test]
fn two_skins_gltf_matches_snapshot() {
    prefab_snapshot(
        TestAssets::fixtures_root,
        "gltf/TwoSkins.gltf",
        "two_skins_gltf",
    );
}
