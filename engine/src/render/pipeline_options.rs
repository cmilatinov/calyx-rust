use crate::render::render_utils::RenderUtils;
use crate::render::RenderContext;
use crate::utils::impl_builder_fn;
use egui_wgpu::wgpu;

pub struct PipelineOptionsBuilder {
    primitive_topology: wgpu::PrimitiveTopology,
    polygon_mode: wgpu::PolygonMode,
    cull_mode: Option<wgpu::Face>,
    fragment_targets: Vec<Option<wgpu::ColorTargetState>>,
    samples: u32,
}

impl PipelineOptionsBuilder {
    impl_builder_fn!(primitive_topology: wgpu::PrimitiveTopology);
    impl_builder_fn!(polygon_mode: wgpu::PolygonMode);
    impl_builder_fn!(cull_mode: Option<wgpu::Face>);
    impl_builder_fn!(fragment_targets: Vec<Option<wgpu::ColorTargetState>>);
    impl_builder_fn!(samples: u32);
    pub fn build(self) -> PipelineOptions {
        PipelineOptions {
            primitive_topology: self.primitive_topology,
            polygon_mode: self.polygon_mode,
            cull_mode: self.cull_mode,
            fragment_targets: self.fragment_targets,
            samples: self.samples,
        }
    }
}

impl Default for PipelineOptionsBuilder {
    fn default() -> Self {
        Self {
            primitive_topology: wgpu::PrimitiveTopology::TriangleList,
            polygon_mode: wgpu::PolygonMode::Fill,
            cull_mode: Some(wgpu::Face::Back),
            fragment_targets: vec![RenderContext::texture_format().map(RenderUtils::color_default)],
            samples: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PipelineOptions {
    pub(crate) primitive_topology: wgpu::PrimitiveTopology,
    pub(crate) polygon_mode: wgpu::PolygonMode,
    pub(crate) cull_mode: Option<wgpu::Face>,
    pub(crate) fragment_targets: Vec<Option<wgpu::ColorTargetState>>,
    pub(crate) samples: u32,
}
