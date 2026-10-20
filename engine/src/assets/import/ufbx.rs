//! FBX and OBJ import through ufbx.

use std::collections::HashMap;
use std::path::Path;

use nalgebra::Unit;
use nalgebra_glm::{Mat4, Vec2, Vec3};

use super::{
    clip_ears, quat, smooth_normals, split_quad, Corner, ImportedAnimation, ImportedBone,
    ImportedMesh, ImportedNode, ImportedScene, MeshBuilder, MAX_UV_CHANNELS,
};
use crate::assets::animation::{AnimationKeyFrames, QuatKeyFrame, VectorKeyFrame};
use crate::assets::error::AssetError;

/// Name given to a root node that has none, as assimp names it.
const ROOT_NODE_NAME: &str = "RootNode";

pub(super) fn import(path: &Path) -> Result<ImportedScene, AssetError> {
    let filename = path
        .to_str()
        .ok_or_else(|| AssetError::LoadError.with_source("path is not valid UTF-8"))?;
    let scene = ufbx::load_file(
        filename,
        ufbx::LoadOpts {
            // Node transforms keep their pivots folded in, and geometry transforms are baked
            // into the vertices, as assimp does with pivot preservation off.
            geometry_transform_handling: ufbx::GeometryTransformHandling::ModifyGeometry,
            ..Default::default()
        },
    )
    .map_err(|error| AssetError::LoadError.with_source(error.description.as_ref()))?;

    let mut meshes = Vec::new();
    let mut mesh_parts = HashMap::new();
    for mesh in scene.meshes.iter() {
        let first = meshes.len();
        meshes.extend(import_mesh(mesh));
        mesh_parts.insert(mesh.element.element_id, first..meshes.len());
    }

    let mut root = import_node(&scene.root_node, &mesh_parts);
    if root.name.is_empty() {
        root.name = ROOT_NODE_NAME.to_string();
    }

    // Stacks that animate no node are skipped, as assimp skips them.
    let animations = scene
        .anim_stacks
        .iter()
        .filter_map(|stack| import_animation(&scene, stack).transpose())
        .collect::<Result<_, _>>()?;

    Ok(ImportedScene {
        meshes,
        root,
        animations,
    })
}

fn import_node(
    node: &ufbx::Node,
    mesh_parts: &HashMap<u32, std::ops::Range<usize>>,
) -> ImportedNode {
    ImportedNode {
        name: node.element.name.to_string(),
        transform: mat4(&node.node_to_parent),
        meshes: node
            .mesh
            .as_ref()
            .and_then(|mesh| mesh_parts.get(&mesh.element.element_id))
            .cloned()
            .map(Iterator::collect)
            .unwrap_or_default(),
        children: node
            .children
            .iter()
            .map(|child| import_node(child, mesh_parts))
            .collect(),
    }
}

/// Imports one mesh per material part, named after the first node using the mesh.
fn import_mesh(mesh: &ufbx::Mesh) -> Vec<ImportedMesh> {
    let name = mesh
        .element
        .instances
        .iter()
        .next()
        .map(|node| node.element.name.to_string())
        .unwrap_or_else(|| mesh.element.name.to_string());
    let uv_sets = mesh
        .uv_sets
        .iter()
        .take(MAX_UV_CHANNELS)
        .map(|set| &set.vertex_uv)
        .collect::<Vec<_>>();
    let all_faces;
    let parts: Vec<&[u32]> = if mesh.material_parts.is_empty() {
        all_faces = (0..mesh.num_faces as u32).collect::<Vec<_>>();
        vec![&all_faces]
    } else {
        mesh.material_parts
            .iter()
            .map(|part| part.face_indices.as_ref())
            .collect()
    };

    parts
        .into_iter()
        .filter_map(|faces| {
            let mut corners = Vec::new();
            for &face in faces {
                let face = mesh.faces[face as usize];
                let polygon =
                    (face.index_begin..face.index_begin + face.num_indices).collect::<Vec<_>>();
                let positions = polygon
                    .iter()
                    .map(|&index| vec3(&mesh.vertex_position[index as usize]))
                    .collect::<Vec<_>>();
                let triangles = match polygon.len() {
                    0..=2 => continue,
                    3 => vec![[0, 1, 2]],
                    4 => split_quad(std::array::from_fn(|i| positions[i])).to_vec(),
                    _ => clip_ears(&positions),
                };
                corners.extend(triangles.into_iter().map(|t| t.map(|i| polygon[i])));
            }
            if corners.is_empty() {
                return None;
            }
            let position = |index: u32| vec3(&mesh.vertex_position[index as usize]);
            let normals = if mesh.vertex_normal.exists {
                corners
                    .iter()
                    .map(|t| t.map(|index| vec3(&mesh.vertex_normal[index as usize])))
                    .collect()
            } else {
                smooth_normals(&corners.iter().map(|t| t.map(position)).collect::<Vec<_>>())
            };

            let skin = mesh.skin_deformers.iter().next();
            let skin_weights = skin.map(|skin| skin_weight_sets(skin)).unwrap_or_default();
            let mut builder = MeshBuilder::new(name.clone(), uv_sets.len());
            for (triangle, normals) in corners.iter().zip(normals) {
                builder.push_triangle([0, 1, 2].map(|i| {
                    let index = triangle[i];
                    let mut uvs = [Vec2::zeros(); MAX_UV_CHANNELS];
                    for (uv, set) in uvs.iter_mut().zip(&uv_sets) {
                        // FBX puts the UV origin at the bottom left; flip it, as assimp's
                        // `FlipUVs` does.
                        let value = &set[index as usize];
                        *uv = Vec2::new(value.x as f32, 1.0 - value.y as f32);
                    }
                    let control_point = mesh.vertex_indices[index as usize];
                    Corner {
                        control_point,
                        skin_weights: skin_weights
                            .get(control_point as usize)
                            .copied()
                            .unwrap_or(0),
                        position: position(index),
                        normal: normals[i],
                        uvs,
                    }
                }));
            }
            let (mut imported, control_points) = builder.finish();
            if let Some(skin) = skin {
                imported.bones = import_skin(skin, &control_points);
            }
            Some(imported)
        })
        .collect()
}

/// Numbers each control point's set of skin weights, giving equal sets the same number.
fn skin_weight_sets(skin: &ufbx::SkinDeformer) -> Vec<u32> {
    let mut sets = HashMap::new();
    skin.vertices
        .iter()
        .map(|vertex| {
            let begin = vertex.weight_begin as usize;
            let weights = skin.weights.as_ref()[begin..begin + vertex.num_weights as usize]
                .iter()
                .map(|weight| (weight.cluster_index, weight.weight.to_bits()))
                .collect::<Vec<_>>();
            let next = sets.len() as u32;
            *sets.entry(weights).or_insert(next)
        })
        .collect()
}

/// One bone per skin cluster, with the weights of the output vertices that came from each
/// influenced control point.
fn import_skin(skin: &ufbx::SkinDeformer, control_points: &[u32]) -> Vec<ImportedBone> {
    let mut bones = skin
        .clusters
        .iter()
        .map(|cluster| ImportedBone {
            name: cluster
                .bone_node
                .as_ref()
                .map(|node| node.element.name.to_string())
                .unwrap_or_else(|| cluster.element.name.to_string()),
            inverse_bind_transform: mat4(&cluster.geometry_to_bone),
            weights: Vec::new(),
        })
        .collect::<Vec<_>>();
    for (vertex, &control_point) in control_points.iter().enumerate() {
        let Some(skin_vertex) = skin.vertices.get(control_point as usize) else {
            continue;
        };
        let begin = skin_vertex.weight_begin as usize;
        for weight in &skin.weights.as_ref()[begin..begin + skin_vertex.num_weights as usize] {
            bones[weight.cluster_index as usize]
                .weights
                .push((vertex as u32, weight.weight as f32));
        }
    }
    bones
}

/// Bakes one animation stack, or returns `None` when it animates no node. Key times start at
/// zero, so a stack whose playback range starts later plays from its first key.
fn import_animation(
    scene: &ufbx::Scene,
    stack: &ufbx::AnimStack,
) -> Result<Option<ImportedAnimation>, AssetError> {
    let opts = ufbx::BakeOpts {
        trim_start_time: true,
        ..Default::default()
    };
    let baked = ufbx::bake_anim(scene, &stack.anim, opts)
        .map_err(|error| AssetError::LoadError.with_source(error.description.as_ref()))?;
    if baked.nodes.is_empty() {
        return Ok(None);
    }
    let channels = baked
        .nodes
        .iter()
        .map(|baked_node| {
            let node = &scene.nodes[baked_node.typed_id as usize];
            let vector_keys = |keys: &ufbx::List<ufbx::BakedVec3>| {
                keys.iter()
                    .map(|key| VectorKeyFrame {
                        value: vec3(&key.value),
                        time: key.time,
                    })
                    .collect()
            };
            (
                node.element.name.to_string(),
                AnimationKeyFrames {
                    positions: vector_keys(&baked_node.translation_keys),
                    rotations: baked_node
                        .rotation_keys
                        .iter()
                        .map(|key| QuatKeyFrame {
                            value: Unit::new_normalize(quat(
                                key.value.x,
                                key.value.y,
                                key.value.z,
                                key.value.w,
                            )),
                            time: key.time,
                        })
                        .collect(),
                    scaling: vector_keys(&baked_node.scale_keys),
                },
            )
        })
        .collect();
    Ok(Some(ImportedAnimation {
        name: stack.element.name.to_string(),
        duration_seconds: baked.playback_duration,
        channels,
    }))
}

fn vec3(v: &ufbx::Vec3) -> Vec3 {
    Vec3::new(v.x as f32, v.y as f32, v.z as f32)
}

fn mat4(m: &ufbx::Matrix) -> Mat4 {
    Mat4::new(
        m.m00 as f32,
        m.m01 as f32,
        m.m02 as f32,
        m.m03 as f32,
        m.m10 as f32,
        m.m11 as f32,
        m.m12 as f32,
        m.m13 as f32,
        m.m20 as f32,
        m.m21 as f32,
        m.m22 as f32,
        m.m23 as f32,
        0.0,
        0.0,
        0.0,
        1.0,
    )
}
