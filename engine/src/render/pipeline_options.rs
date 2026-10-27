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
///
/// Every field is part of the key [`crate::render::Shader::pipeline`] caches
/// on, so two options that differ in any field build two pipelines.
#[derive(Debug, Clone, PartialEq, Eq, Hash, TypedBuilder)]
pub struct PipelineOptions {
    /// How vertices assemble into primitives: triangles, lines or points.
    /// Defaults to a triangle list; gizmos also draw lines.
    #[builder(default = wgpu::PrimitiveTopology::TriangleList)]
    pub(crate) primitive_topology: wgpu::PrimitiveTopology,
    /// How triangles are rasterized: filled, or as their edges for a
    /// wireframe. Anything but `Fill` needs the matching device feature.
    #[builder(default = wgpu::PolygonMode::Fill)]
    pub(crate) polygon_mode: wgpu::PolygonMode,
    /// Which triangle face is discarded, with counter-clockwise triangles
    /// facing front. Defaults to back faces. `None` draws both sides, as
    /// particles and gizmos need; the skybox culls front faces so it is seen
    /// from inside.
    #[builder(default = Some(wgpu::Face::Back))]
    pub(crate) cull_mode: Option<wgpu::Face>,
    /// One entry per color attachment of the render pass, in attachment
    /// order: its format, blending and write mask. The formats must match
    /// the pass's attachments. Empty draws no color, as a depth-only pass.
    #[builder(default = vec![])]
    pub(crate) fragment_targets: Vec<Option<wgpu::ColorTargetState>>,
    /// Depth attachment format and how fragments test against and write
    /// depth. Must match the pass's depth attachment, or be `None` for a pass
    /// without one, such as the outline. Defaults to a `Depth32Float` less-than
    /// test that writes depth.
    #[builder(default = Some(RenderUtils::depth_default(wgpu::TextureFormat::Depth32Float)))]
    pub(crate) depth_stencil: Option<wgpu::DepthStencilState>,
    /// MSAA sample count. Must equal the sample count of the pass's
    /// attachments; values above one must be supported by the adapter for
    /// those formats.
    #[builder(default = 1)]
    pub(crate) samples: u32,
    /// Vertex buffer layouts, one per vertex buffer slot in slot order, each
    /// stepping per vertex or per instance. Defaults to the mesh vertex
    /// layout; particles add a per-instance layout in slot 1.
    #[builder(default = vec![VertexLayout::of::<mesh::Vertex>(wgpu::VertexStepMode::Vertex)])]
    pub(crate) vertex_layouts: Vec<VertexLayout>,
}

impl PipelineOptions {
    /// Base options of a full-screen draw: the triangle of
    /// `shaders/postprocess/fullscreen.wgsl`, drawn by
    /// [`RenderUtils::draw_fullscreen`] from no vertex buffers. Culling is
    /// off since the triangle winds clockwise. It has no color targets and
    /// no depth; callers add theirs with struct update syntax.
    pub fn fullscreen() -> Self {
        Self::builder()
            .cull_mode(None)
            .vertex_layouts(vec![])
            .depth_stencil(None)
            .fragment_targets(vec![])
            .build()
    }
}
