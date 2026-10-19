use std::cmp::min;
use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;

use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt;
use nalgebra_glm::{vec2, vec3, vec4, IVec4, Mat4, Vec2, Vec3, Vec4};
use russimp_ng::scene::{PostProcess, Scene};

use super::LoadedAsset;
use crate::assets::error::AssetError;
use crate::assets::Asset;
use crate::context::ReadOnlyAssetContext;
use crate::render::buffer::{wgpu_buffer_init_desc, BufferLayout, ResizableBuffer};
use crate::render::RenderContext;
use crate::utils::TypeUuid;
use crate::{self as engine, math};

const CX_MESH_NUM_UV_CHANNELS: usize = 4;

/// Packed GPU vertex layout used by mesh rendering.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv0: [f32; 2],
    uv1: [f32; 2],
    uv2: [f32; 2],
    uv3: [f32; 2],
    bone_indices: [i32; 4],
    bone_weights: [f32; 4],
}

impl Vertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 8] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x3,
        2 => Float32x2,
        3 => Float32x2,
        4 => Float32x2,
        5 => Float32x2,
        6 => Sint32x4,
        7 => Float32x4
    ];
}

impl BufferLayout for Vertex {
    const ATTRIBS: &'static [wgpu::VertexAttribute] = &Vertex::ATTRIBUTES;
}

/// Uniform payload uploaded for mesh instance and skinning data.
#[repr(C)]
#[derive(Default, Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshUniforms {
    /// Number of bones stored in the bone buffer.
    pub num_bones: u32,
    /// Padding for alignment.
    pub _padding: [u32; 3],
    /// Per-instance transform data.
    pub instances: [Instance; Mesh::MAX_INSTANCES],
}

/// Per-instance mesh draw data.
#[repr(C)]
#[derive(Default, Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    /// Starting index into the shared bone transform buffer.
    pub bone_transform_index: i32,
    /// Editor/runtime object id associated with this draw instance.
    pub object_id: u32,
    /// Padding for alignment.
    pub _padding: [u32; 2],
    /// Model transform matrix.
    pub transform: [[f32; 4]; 4],
}

/// Skinning transform uploaded for one bone.
#[repr(C)]
#[derive(Default, Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BoneTransform {
    /// Final bone matrix used by the skinning shader.
    pub transform: [[f32; 4]; 4],
}

/// Skeleton metadata for one mesh bone.
#[derive(Debug)]
pub struct BoneInfo {
    /// Bone index in the mesh skeleton.
    pub index: usize,
    /// Inverse bind pose matrix.
    pub inverse_bind_transform: Mat4,
}

/// Mesh asset containing geometry, skinning, and GPU upload state.
#[derive(TypeUuid)]
#[uuid = "792d264b-de6f-4431-b59f-76f18fdb3bfe"]
pub struct Mesh {
    /// Triangle index buffer.
    pub indices: Vec<u32>,
    /// Vertex positions.
    pub vertices: Vec<Vec3>,
    /// Vertex normals.
    pub normals: Vec<Vec3>,
    /// Up to four UV channels.
    pub uvs: [Vec<Vec2>; CX_MESH_NUM_UV_CHANNELS],
    /// Bone indices per vertex.
    pub bone_indices: Vec<IVec4>,
    /// Bone weights per vertex.
    pub bone_weights: Vec<Vec4>,
    /// Bone metadata keyed by bone name.
    pub bones: HashMap<String, BoneInfo>,

    pub(crate) dirty: bool,
    pub(crate) instances: Vec<Instance>,
    pub(crate) bone_transforms: Vec<BoneTransform>,
    pub(crate) index_buffer: Option<wgpu::Buffer>,
    pub(crate) vertex_buffer: Option<wgpu::Buffer>,
    /// One uniform buffer per `MAX_INSTANCES`-sized chunk of `instances`.
    pub(crate) instance_buffers: Vec<wgpu::Buffer>,
    pub(crate) bone_buffer: ResizableBuffer,

    pub(crate) instance_bind_group_layout: wgpu::BindGroupLayout,
}

impl Mesh {
    /// Maximum instanced draws packed into one uniform buffer update.
    pub const MAX_INSTANCES: usize = 30;
    /// Maximum number of bones expected by the skinning path.
    pub const MAX_BONES: usize = 100;
    // const ATTRIBUTE_VERTEX: u32 = 0;
    // const ATTRIBUTE_NORMAL: u32 = 1;
    // const ATTRIBUTE_UV0: u32 = 2;
    // const ATTRIBUTE_UV1: u32 = 3;
    // const ATTRIBUTE_UV2: u32 = 4;
    // const ATTRIBUTE_UV3: u32 = 5;
    // const ATTRIBUTE_BONE_INDICES: u32 = 6;
    // const ATTRIBUTE_BONE_WEIGHTS: u32 = 7;
    // const ATTRIBUTE_MODEL0: u32 = 6;
    // const ATTRIBUTE_MODEL1: u32 = 7;
    // const ATTRIBUTE_MODEL2: u32 = 8;
    // const ATTRIBUTE_MODEL3: u32 = 9;
}

impl Mesh {
    /// Creates an empty mesh with GPU-side buffers initialized for
    /// `render_context`.
    pub fn new(render_context: &RenderContext) -> Self {
        let device = render_context.device();

        let instance_buffers = vec![Self::create_instance_buffer(device)];

        let instance_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("instance_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        Self {
            indices: Default::default(),
            vertices: Default::default(),
            normals: Default::default(),
            uvs: [
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
            ],
            bone_indices: Default::default(),
            bone_weights: Default::default(),
            bones: Default::default(),
            dirty: false,
            instances: Default::default(),
            bone_transforms: Default::default(),
            index_buffer: None,
            vertex_buffer: None,
            instance_buffers,
            bone_buffer: ResizableBuffer::new(
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::STORAGE,
            ),
            instance_bind_group_layout,
        }
    }
}

impl Asset for Mesh {
    fn asset_name() -> &'static str
    where
        Self: Sized,
    {
        "Mesh"
    }

    fn file_extensions() -> &'static [&'static str] {
        &["obj"]
    }

    fn from_file(
        game: &ReadOnlyAssetContext,
        path: &Path,
    ) -> Result<LoadedAsset<Self>, AssetError> {
        let scene_path = path.to_str().ok_or_else(|| {
            AssetError::LoadError
                .with_path(path)
                .with_type(Self::asset_name())
                .with_source("path is not valid UTF-8")
        })?;
        let scene = Scene::from_file(
            scene_path,
            vec![
                PostProcess::Triangulate,
                PostProcess::GenerateSmoothNormals,
                PostProcess::FlipUVs,
                PostProcess::FlipWindingOrder,
                PostProcess::JoinIdenticalVertices,
            ],
        )
        .map_err(|err| {
            AssetError::from(err)
                .with_path(path)
                .with_type(Self::asset_name())
        })?;

        // Assuming you want to load the first mesh in the scene
        let mesh = scene.meshes.first().ok_or(
            AssetError::NotFound
                .with_path(path)
                .with_type(Self::asset_name()),
        )?;
        log::info!(
            "Loaded mesh {} ({} vertices, {} faces, {} bones)",
            path.display(),
            mesh.vertices.len(),
            mesh.faces.len(),
            mesh.bones.len()
        );
        Ok(LoadedAsset::new(Mesh::from_russimp_mesh(
            &game.render_context,
            mesh,
        )))
    }
}

impl Mesh {
    fn insert_bone_weight(
        bone_index: i32,
        weight: f32,
        bone_ids: &mut [i32],
        bone_weights: &mut [f32],
    ) {
        if let Some(index) = bone_weights
            .iter()
            .enumerate()
            .find(|(i, w)| bone_ids[*i] == -1 || **w < weight)
            .map(|(i, _)| i)
        {
            if bone_ids[index] != -1 {
                bone_ids[index..].rotate_right(1);
                bone_weights[index..].rotate_right(1);
            }
            bone_ids[index] = bone_index;
            bone_weights[index] = weight;
        }
    }

    /// Keeps the four heaviest bone influences of each vertex, normalised to sum to one.
    /// `bones` yields each bone's `(vertex, weight)` influences, in bone index order.
    fn bind_bone_weights(
        vertex_count: usize,
        bones: impl IntoIterator<Item = impl IntoIterator<Item = (u32, f32)>>,
    ) -> (Vec<IVec4>, Vec<Vec4>) {
        let mut bone_indices = vec![vec4::<i32>(-1, -1, -1, -1); vertex_count];
        let mut bone_weights = vec![vec4(0.0, 0.0, 0.0, 0.0); vertex_count];
        for (bone_index, weights) in bones.into_iter().enumerate() {
            for (vertex, weight) in weights {
                Self::insert_bone_weight(
                    bone_index as i32,
                    weight,
                    bone_indices[vertex as usize].as_mut_slice(),
                    bone_weights[vertex as usize].as_mut_slice(),
                );
            }
        }
        for (indices, weights) in bone_indices.iter().zip(bone_weights.iter_mut()) {
            let sum: f32 = indices
                .iter()
                .zip(weights.iter())
                .filter(|(index, _)| **index >= 0)
                .map(|(_, weight)| *weight)
                .sum();
            if sum > 0.00001 {
                *weights /= sum;
            }
        }
        (bone_indices, bone_weights)
    }

    /// Builds a mesh asset from a Russimp mesh.
    pub fn from_russimp_mesh(
        render_context: &RenderContext,
        mesh: &russimp_ng::mesh::Mesh,
    ) -> Self {
        let indices = mesh
            .faces
            .iter()
            .flat_map(|face| face.0.iter().cloned())
            .collect();
        let mut vertices = vec![Vec3::zeros(); mesh.vertices.len()];
        let mut normals = vec![Vec3::zeros(); mesh.vertices.len()];

        let num_uvs: usize = min(mesh.uv_components.len(), CX_MESH_NUM_UV_CHANNELS);
        let mut uvs = [
            vec![Vec2::zeros(); mesh.vertices.len()],
            vec![Vec2::zeros(); mesh.vertices.len()],
            vec![Vec2::zeros(); mesh.vertices.len()],
            vec![Vec2::zeros(); mesh.vertices.len()],
        ];

        for (i, vertex) in mesh.vertices.iter().enumerate() {
            vertices[i] = vec3(vertex.x, vertex.y, vertex.z);
            normals[i] = vec3(mesh.normals[i].x, mesh.normals[i].y, mesh.normals[i].z);
            for (j, coord) in uvs.iter_mut().enumerate().take(num_uvs) {
                if let Some(tex_coord) = mesh.texture_coords[j].as_ref() {
                    coord[i] = vec2(tex_coord[i].x, tex_coord[i].y);
                }
            }
        }

        let bones = mesh
            .bones
            .iter()
            .enumerate()
            .map(|(bone_index, bone)| {
                (
                    bone.name.clone(),
                    BoneInfo {
                        index: bone_index,
                        inverse_bind_transform: math::mat4_from_russimp(&bone.offset_matrix),
                    },
                )
            })
            .collect();
        let (bone_indices, bone_weights) = Self::bind_bone_weights(
            mesh.vertices.len(),
            mesh.bones.iter().map(|bone| {
                bone.weights
                    .iter()
                    .map(|weight| (weight.vertex_id, weight.weight))
            }),
        );

        Self {
            indices,
            vertices,
            normals,
            uvs,
            bones,
            bone_indices,
            bone_weights,
            dirty: true,
            ..Mesh::new(render_context)
        }
    }
}

impl Mesh {
    /// Clears all CPU-side geometry arrays.
    pub fn clear(&mut self) {
        self.indices.clear();
        self.vertices.clear();
        self.normals.clear();
        self.instances.clear();
        for uv in &mut self.uvs {
            uv.clear();
        }
    }

    /// Rebuilds the GPU index buffer from `self.indices`.
    pub fn rebuild_index_buffer(&mut self, device: &wgpu::Device) {
        self.index_buffer = Some(device.create_buffer_init(&wgpu_buffer_init_desc(
            wgpu::BufferUsages::INDEX,
            self.indices.as_slice(),
        )));
    }

    fn rebuild_vertex_buffer(&mut self, device: &wgpu::Device) {
        let vertex_count = self.vertices.len();
        let mut vertices: Vec<Vertex> = Vec::new();
        vertices.resize(vertex_count, Vertex::default());
        for (i, vertex) in vertices.iter_mut().enumerate().take(vertex_count) {
            vertex.position = self.vertices[i].into();
            vertex.normal = self.normals[i].into();
            vertex.uv0 = self.uvs[0][i].into();
            vertex.uv1 = self.uvs[1][i].into();
            vertex.uv2 = self.uvs[2][i].into();
            vertex.uv3 = self.uvs[3][i].into();
            vertex.bone_indices = self.bone_indices[i].into();
            vertex.bone_weights = self.bone_weights[i].into();
        }
        self.vertex_buffer = Some(device.create_buffer_init(&wgpu_buffer_init_desc(
            wgpu::BufferUsages::VERTEX,
            vertices.as_slice(),
        )));
    }

    fn create_instance_buffer(device: &wgpu::Device) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instance_buffer"),
            size: (std::mem::size_of::<MeshUniforms>()) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: false,
        })
    }

    /// Maps a range of mesh instance indices that does not cross a chunk
    /// boundary to its chunk index and the range local to that chunk.
    pub(crate) fn instance_chunk(range: &Range<u32>) -> (usize, Range<u32>) {
        let cap = Self::MAX_INSTANCES as u32;
        let offset = range.start % cap;
        debug_assert!(
            range.is_empty() || (range.end - 1) / cap == range.start / cap,
            "instance range {range:?} crosses an instance chunk boundary"
        );
        (
            (range.start / cap) as usize,
            offset..(offset + range.end - range.start),
        )
    }

    fn rebuild_instance_buffer(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let chunk_count = self.instances.len().div_ceil(Self::MAX_INSTANCES).max(1);
        self.instance_buffers.truncate(chunk_count);
        while self.instance_buffers.len() < chunk_count {
            self.instance_buffers
                .push(Self::create_instance_buffer(device));
        }
        for (i, buffer) in self.instance_buffers.iter().enumerate() {
            let mut instances: [Instance; Self::MAX_INSTANCES] = Default::default();
            for (slot, instance) in instances
                .iter_mut()
                .zip(self.instances.iter().skip(i * Self::MAX_INSTANCES))
            {
                *slot = *instance;
            }
            let uniforms = MeshUniforms {
                num_bones: self.bones.len() as u32,
                _padding: Default::default(),
                instances,
            };
            queue.write_buffer(
                buffer,
                0 as wgpu::BufferAddress,
                bytemuck::cast_slice([uniforms].as_slice()),
            );
        }
    }

    fn rebuild_bone_buffer(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.bone_buffer.resize(
            device,
            16 + (self.bone_transforms.len() * std::mem::size_of::<BoneTransform>())
                .max(std::mem::size_of::<BoneTransform>()) as u64,
        );
        self.bone_buffer
            .write_buffer(device, queue, &[self.bone_transforms.len() as u32], None);
        self.bone_buffer
            .write_buffer(device, queue, &self.bone_transforms, Some(16));
    }

    fn normalize_mesh_data(&mut self) {
        let vertex_count = self.vertices.len();
        self.normals.resize(vertex_count, Vec3::zeros());
        for i in 0..CX_MESH_NUM_UV_CHANNELS {
            self.uvs[i].resize(vertex_count, Vec2::zeros());
        }
        self.bone_indices
            .resize(vertex_count, IVec4::new(-1, -1, -1, -1));
        self.bone_weights.resize(vertex_count, Vec4::zeros());
    }

    pub(crate) fn rebuild_mesh_data(&mut self, device: &wgpu::Device) {
        self.normalize_mesh_data();
        self.rebuild_index_buffer(device);
        self.rebuild_vertex_buffer(device);
    }

    pub(crate) fn rebuild_instance_data(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.rebuild_instance_buffer(device, queue);
        self.rebuild_bone_buffer(device, queue);
    }

    /// Creates one instance bind group per instance buffer chunk.
    pub(crate) fn instance_bind_groups(&self, device: &wgpu::Device) -> Vec<wgpu::BindGroup> {
        self.instance_buffers
            .iter()
            .map(|instance_buffer| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("instance_bind_group"),
                    layout: &self.instance_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: instance_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: self.bone_buffer.get_wgpu_buffer().as_entire_binding(),
                        },
                    ],
                })
            })
            .collect()
    }

    /// Marks the mesh so its GPU buffers are rebuilt on the next render pass.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::Mesh;
    use nalgebra_glm::{IVec4, Vec4};

    #[test]
    fn heavier_bone_weight_shifts_lighter_ones_with_their_bones() {
        let mut bone_ids = [-1; 4];
        let mut bone_weights = [0.0; 4];

        Mesh::insert_bone_weight(0, 0.3, &mut bone_ids, &mut bone_weights);
        Mesh::insert_bone_weight(1, 0.7, &mut bone_ids, &mut bone_weights);

        assert_eq!(bone_ids, [1, 0, -1, -1]);
        assert_eq!(bone_weights, [0.7, 0.3, 0.0, 0.0]);
    }

    #[test]
    fn bone_weights_are_normalised_once_every_bone_is_bound() {
        let (bone_indices, bone_weights) =
            Mesh::bind_bone_weights(1, [vec![(0, 0.15)], vec![(0, 0.35)]]);

        assert_eq!(bone_indices[0], IVec4::new(1, 0, -1, -1));
        assert!((bone_weights[0] - Vec4::new(0.7, 0.3, 0.0, 0.0)).norm() < 1e-6);
    }
}
