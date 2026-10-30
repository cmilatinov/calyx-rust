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

/// Pipeline options of a draw of `shaders/common/fullscreen.wgsl`'s triangle, which
/// [`RenderUtils::draw_fullscreen`] draws from no vertex buffers.
///
/// Only what a full-screen draw varies is settable: there are no vertex
/// buffers, culling is off since the triangle winds clockwise, and there is
/// no depth unless set. It builds into [`PipelineOptions`]; start one with
/// [`PipelineOptions::fullscreen_builder`].
#[derive(TypedBuilder)]
#[builder(build_method(into = PipelineOptions))]
pub struct FullscreenPipelineOptions {
    /// The one color target, if any: its format, blending and write mask.
    /// Without one the draw writes no color.
    #[builder(default, setter(strip_option))]
    target: Option<wgpu::ColorTargetState>,
    /// Depth attachment format and how fragments test against and write
    /// depth. Defaults to `None`, for a pass without a depth attachment.
    #[builder(default, setter(strip_option))]
    depth_stencil: Option<wgpu::DepthStencilState>,
    /// MSAA sample count, as [`PipelineOptions`]' `samples`.
    #[builder(default = 1)]
    samples: u32,
}

impl From<FullscreenPipelineOptions> for PipelineOptions {
    fn from(options: FullscreenPipelineOptions) -> Self {
        Self::builder()
            .cull_mode(None)
            .vertex_layouts(vec![])
            .fragment_targets(options.target.into_iter().map(Some).collect())
            .depth_stencil(options.depth_stencil)
            .samples(options.samples)
            .build()
    }
}

impl PipelineOptions {
    /// Starts the options of a full-screen draw, which can set only its color
    /// target, depth and sample count. See [`FullscreenPipelineOptions`].
    pub fn fullscreen_builder() -> FullscreenPipelineOptionsBuilder {
        FullscreenPipelineOptions::builder()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fullscreen_builder_matches_the_equivalent_pipeline_options() {
        let target = RenderUtils::color_alpha_blending(wgpu::TextureFormat::Rgba16Float);
        let depth = wgpu::DepthStencilState {
            depth_write_enabled: false,
            ..RenderUtils::depth_default(wgpu::TextureFormat::Depth32Float)
        };
        let built = PipelineOptions::fullscreen_builder()
            .samples(4)
            .target(target.clone())
            .depth_stencil(depth.clone())
            .build();
        let expected = PipelineOptions::builder()
            .cull_mode(None)
            .vertex_layouts(vec![])
            .fragment_targets(vec![Some(target)])
            .depth_stencil(Some(depth))
            .samples(4)
            .build();
        assert_eq!(built, expected);
    }

    #[test]
    fn fullscreen_builder_defaults_to_no_depth_culling_or_vertex_layouts() {
        let options = PipelineOptions::fullscreen_builder().build();
        assert_eq!(options.depth_stencil, None);
        assert_eq!(options.cull_mode, None);
        assert!(options.vertex_layouts.is_empty());
        assert!(options.fragment_targets.is_empty());
        assert_eq!(options.samples, 1);
    }
}
