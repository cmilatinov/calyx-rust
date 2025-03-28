use crate::render::RenderUtils;
use derive_builder::Builder;
use egui_wgpu::wgpu;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Builder)]
#[builder(pattern = "owned")]
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
}
