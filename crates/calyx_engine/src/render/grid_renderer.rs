use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::overlay::{OverlayContext, OverlayPass};
use crate::render::render_utils::RenderUtils;
use crate::render::{PipelineOptions, SceneRendererError, Shader};
use egui_wgpu::wgpu;

pub struct GridRenderer {
    shader: Ref<Shader>,
    camera_uniform_buffer: wgpu::Buffer,
    /// The camera bind group with the [`Shader::layout_generation`] it was
    /// built for, or `None` for a layout it could not be built for.
    camera_bind_group: Option<(u64, Option<wgpu::BindGroup>)>,
}

impl GridRenderer {
    pub fn new(
        context: &ReadOnlyAssetContext,
        camera_uniform_buffer: &wgpu::Buffer,
    ) -> Result<Self, SceneRendererError> {
        let shader = SceneRendererError::load::<Shader>(context, "shaders/editor/grid")?;
        let mut renderer = Self {
            shader,
            camera_uniform_buffer: camera_uniform_buffer.clone(),
            camera_bind_group: None,
        };
        // Built now, so the shader knows from the start which resource its
        // renderer binds and a hot reload may renumber it.
        renderer.camera_bind_group();
        Ok(renderer)
    }

    /// The camera bind group for the shader's current layout, rebuilt after a
    /// hot reload changes it, or `None` when it cannot be built.
    pub fn camera_bind_group(&mut self) -> Option<wgpu::BindGroup> {
        let shader = self.shader.read();
        let generation = shader.layout_generation();
        if self
            .camera_bind_group
            .as_ref()
            .is_none_or(|(built, _)| *built != generation)
        {
            let bind_group = shader.create_bind_group(
                0,
                Some("camera_bind_group"),
                &[("camera", self.camera_uniform_buffer.as_entire_binding())],
            );
            self.camera_bind_group = Some((generation, bind_group));
        }
        self.camera_bind_group
            .as_ref()
            .and_then(|(_, bind_group)| bind_group.clone())
    }
}

impl OverlayPass for GridRenderer {
    fn name(&self) -> &str {
        "grid"
    }

    fn enabled(&self, context: &OverlayContext) -> bool {
        context.options.grid
    }

    fn prepare(&mut self, _context: &OverlayContext) {}

    fn record(&mut self, context: &OverlayContext, render_pass: &mut wgpu::RenderPass<'_>) {
        let options = PipelineOptions::fullscreen_builder()
            .samples(context.samples)
            .target(RenderUtils::color_alpha_blending(context.format))
            .depth_stencil(wgpu::DepthStencilState {
                depth_write_enabled: false,
                ..RenderUtils::depth_default(wgpu::TextureFormat::Depth32Float)
            })
            .build();
        let Some(camera_bind_group) = self.camera_bind_group() else {
            return;
        };
        let Some(pipeline) = self.shader.read().pipeline(&options) else {
            return;
        };
        render_pass.set_pipeline(&pipeline);
        render_pass.set_bind_group(0, &camera_bind_group, &[]);
        RenderUtils::draw_fullscreen(render_pass);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestAssets;
    use egui_wgpu::wgpu::util::DeviceExt;

    #[test]
    fn creates_camera_bind_group_from_grid_shader_layout() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let device = read_only_context.render_context.device();
        let camera_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("test_camera_uniform_buffer"),
            contents: &[0; 272],
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let mut renderer = GridRenderer::new(&read_only_context, &camera_uniform_buffer)
            .expect("grid renderer should load its shader");

        assert!(renderer.camera_bind_group().is_some());
    }
}
