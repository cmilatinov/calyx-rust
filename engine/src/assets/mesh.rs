use std::collections::HashMap;
use std::path::Path;

use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt;
use nalgebra_glm::{vec4, IVec4, Mat4, Vec2, Vec3, Vec4};

use super::LoadedAsset;
use crate as engine;
use crate::assets::error::AssetError;
use crate::assets::import::{self, ImportedMesh};
use crate::assets::Asset;
use crate::context::ReadOnlyAssetContext;
use crate::render::buffer::{wgpu_buffer_init_desc, BufferLayout, ResizableBuffer};
use crate::render::RenderContext;
use crate::utils::TypeUuid;

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

/// Header of the mesh instance storage buffer, followed by its instances.
#[repr(C)]
#[derive(Default, Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshInstancesHeader {
    /// Number of bones stored in the bone buffer.
    pub num_bones: u32,
    /// Padding to the 16-byte alignment of the instance array.
    pub _padding: [u32; 3],
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
    /// Storage buffer holding a [`MeshInstancesHeader`] followed by every
    /// entry of `instances`.
    pub(crate) instance_buffer: ResizableBuffer,
    pub(crate) bone_buffer: ResizableBuffer,

    pub(crate) instance_bind_group_layout: wgpu::BindGroupLayout,
    /// Bind group over `instance_buffer` and `bone_buffer`, reused across
    /// frames, with the `(instance, bone)` buffer generations it was built
    /// over.
    pub(crate) instance_bind_group: Option<(wgpu::BindGroup, (u64, u64))>,
}

impl Mesh {
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

        let instance_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("instance_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
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
            instance_buffer: ResizableBuffer::new(
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::STORAGE,
            ),
            bone_buffer: ResizableBuffer::new(
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::STORAGE,
            ),
            instance_bind_group_layout,
            instance_bind_group: None,
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
        let scene = import::import(path).map_err(|err| err.with_type(Self::asset_name()))?;

        // A mesh asset holds the whole file: every object and material part is merged into one.
        let mesh = ImportedMesh::merge(scene.meshes).ok_or(
            AssetError::NotFound
                .with_path(path)
                .with_type(Self::asset_name()),
        )?;
        log::info!(
            "Loaded mesh {} ({} vertices, {} faces, {} bones)",
            path.display(),
            mesh.positions.len(),
            mesh.indices.len() / 3,
            mesh.bones.len()
        );
        Ok(LoadedAsset::new(Mesh::from_imported(
            &game.render_context,
            &mesh,
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

    /// Builds a mesh asset from an imported mesh.
    pub(crate) fn from_imported(render_context: &RenderContext, mesh: &ImportedMesh) -> Self {
        let indices = mesh.indices.clone();
        let vertices = mesh.positions.clone();
        let normals = mesh.normals.clone();
        let mut uvs: [Vec<Vec2>; CX_MESH_NUM_UV_CHANNELS] =
            std::array::from_fn(|_| vec![Vec2::zeros(); mesh.positions.len()]);
        for (channel, imported) in uvs.iter_mut().zip(&mesh.uvs) {
            channel.clone_from(imported);
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
                        inverse_bind_transform: bone.inverse_bind_transform,
                    },
                )
            })
            .collect();
        let (bone_indices, bone_weights) = Self::bind_bone_weights(
            mesh.positions.len(),
            mesh.bones.iter().map(|bone| bone.weights.iter().copied()),
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

    fn rebuild_instance_buffer(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let header_size = std::mem::size_of::<MeshInstancesHeader>() as u64;
        // Sized up front so the header written first survives a growth, and
        // for at least one instance: the binding must hold the header and one
        // element of the runtime-sized instance array.
        self.instance_buffer.resize(
            device,
            header_size + (self.instances.len().max(1) * std::mem::size_of::<Instance>()) as u64,
        );
        let header = MeshInstancesHeader {
            num_bones: self.bones.len() as u32,
            _padding: Default::default(),
        };
        self.instance_buffer
            .write_buffer(device, queue, &[header], None);
        self.instance_buffer
            .write_buffer(device, queue, &self.instances, Some(header_size));
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
        self.refresh_instance_bind_group(device);
    }

    /// Keeps the instance bind group, rebuilding it only when a buffer it
    /// binds was reallocated.
    fn refresh_instance_bind_group(&mut self, device: &wgpu::Device) {
        let generations = (
            self.instance_buffer.generation(),
            self.bone_buffer.generation(),
        );
        if matches!(&self.instance_bind_group, Some((_, cached)) if *cached == generations) {
            return;
        }
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("instance_bind_group"),
            layout: &self.instance_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.instance_buffer.get_wgpu_buffer().as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.bone_buffer.get_wgpu_buffer().as_entire_binding(),
                },
            ],
        });
        self.instance_bind_group = Some((bind_group, generations));
    }

    /// The bind group over the instance and bone storage buffers, built by
    /// the last [`Self::rebuild_instance_data`]. Draws index its instances
    /// with ranges over the whole of `instances`.
    pub(crate) fn instance_bind_group(&self) -> Option<&wgpu::BindGroup> {
        self.instance_bind_group.as_ref().map(|(group, _)| group)
    }

    /// Marks the mesh so its GPU buffers are rebuilt on the next render pass.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::{BoneTransform, Instance, Mesh};
    use crate::assets::Asset;
    use crate::test_support::{test_render_context, TestAssets};
    use nalgebra_glm::{IVec4, Vec3, Vec4};

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

    #[test]
    fn every_object_and_material_of_an_obj_is_imported() {
        // A textured quad, and an untextured quad split across two materials.
        let assets = TestAssets::models();
        let context = assets.asset_context();
        let context = context.lock_read();
        let path = assets.fixtures_root().join("obj/TwoObjects.obj");
        let mesh = Mesh::from_file(&context, &path)
            .unwrap_or_else(|e| panic!("failed to import the two objects: {e}"))
            .asset;

        assert_eq!(mesh.vertices.len(), 10);
        assert_eq!(mesh.indices.len(), 4 * 3);
        assert!(mesh
            .indices
            .iter()
            .all(|&index| (index as usize) < mesh.vertices.len()));
        let (min, max) = mesh.vertices.iter().fold(
            (Vec3::repeat(f32::INFINITY), Vec3::repeat(f32::NEG_INFINITY)),
            |(min, max), p| (min.inf(p), max.sup(p)),
        );
        assert_eq!((min, max), (Vec3::zeros(), Vec3::new(3.0, 1.0, 1.0)));
        assert_eq!(mesh.normals.len(), mesh.vertices.len());
        assert!(mesh
            .uvs
            .iter()
            .all(|channel| channel.len() == mesh.vertices.len()));
    }

    #[test]
    fn instance_bind_group_is_reused_until_a_buffer_is_reallocated() {
        let render_context = test_render_context();
        let (device, queue) = (render_context.device(), render_context.queue());
        let mut mesh = Mesh::new(&render_context);
        mesh.rebuild_instance_data(device, queue);
        let first = mesh
            .instance_bind_group()
            .cloned()
            .expect("the mesh has an instance bind group");

        mesh.rebuild_instance_data(device, queue);
        assert_eq!(
            mesh.instance_bind_group(),
            Some(&first),
            "unchanged buffers"
        );

        mesh.instances = vec![Instance::default(); 1000];
        mesh.rebuild_instance_data(device, queue);
        let grown = mesh
            .instance_bind_group()
            .cloned()
            .expect("the grown buffer has a bind group");
        assert_ne!(grown, first, "a grown instance buffer rebuilds the group");

        mesh.instances.truncate(1);
        mesh.rebuild_instance_data(device, queue);
        assert_eq!(
            mesh.instance_bind_group(),
            Some(&grown),
            "fewer instances fit the buffer as it is"
        );

        mesh.bone_transforms = vec![BoneTransform::default(); Mesh::MAX_BONES];
        mesh.rebuild_instance_data(device, queue);
        assert_ne!(
            mesh.instance_bind_group(),
            Some(&grown),
            "a reallocated bone buffer rebuilds the group"
        );
    }
}
