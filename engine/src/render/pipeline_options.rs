use crate::assets::mesh;
use crate::render::buffer::BufferLayout;
use crate::render::RenderUtils;
use egui_wgpu::wgpu;
use typed_builder::TypedBuilder;

/// Owned, hashable vertex buffer layout, so pipeline options can key a cache
/// on the vertex inputs a pipeline expects.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VertexLayout {
    /// Byte stride between consecutive elements.
    pub array_stride: wgpu::BufferAddress,
    /// Whether the buffer advances per vertex or per instance.
    pub step_mode: wgpu::VertexStepMode,
    /// Attributes read from each element.
    pub attributes: Vec<wgpu::VertexAttribute>,
}

impl VertexLayout {
    /// Builds the layout of the vertex type `T`.
    pub fn of<T: BufferLayout>(step_mode: wgpu::VertexStepMode) -> Self {
        let layout = T::layout(step_mode);
        Self {
            array_stride: layout.array_stride,
            step_mode: layout.step_mode,
            attributes: layout.attributes.to_vec(),
        }
    }

    /// Borrows this layout as a wgpu vertex buffer layout.
    pub fn as_wgpu(&self) -> wgpu::VertexBufferLayout<'_> {
        wgpu::VertexBufferLayout {
            array_stride: self.array_stride,
            step_mode: self.step_mode,
            attributes: self.attributes.as_slice(),
        }
    }
}

/// Render-pipeline configuration used when building shader pipelines.
#[derive(Debug, Clone, PartialEq, Eq, Hash, TypedBuilder)]
pub struct PipelineOptions {
    #[builder(default = wgpu::PrimitiveTopology::TriangleList)]
    pub(crate) primitive_topology: wgpu::PrimitiveTopology,
    #[builder(default = wgpu::PolygonMode::Fill)]
    pub(crate) polygon_mode: wgpu::PolygonMode,
    #[builder(default = Some(wgpu::Face::Back))]
    pub(crate) cull_mode: Option<wgpu::Face>,
    #[builder(default = vec![])]
    pub(crate) fragment_targets: Vec<Option<wgpu::ColorTargetState>>,
    #[builder(default = Some(RenderUtils::depth_default(wgpu::TextureFormat::Depth32Float)))]
    pub(crate) depth_stencil: Option<wgpu::DepthStencilState>,
    #[builder(default = 1)]
    pub(crate) samples: u32,
    /// Vertex buffer layouts, one per vertex buffer slot. Defaults to the
    /// mesh vertex layout.
    #[builder(default = vec![VertexLayout::of::<mesh::Vertex>(wgpu::VertexStepMode::Vertex)])]
    pub(crate) vertex_layouts: Vec<VertexLayout>,
}
