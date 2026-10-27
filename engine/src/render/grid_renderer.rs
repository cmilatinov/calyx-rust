use crate::assets::texture::Texture;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::render_utils::RenderUtils;
use crate::render::{PipelineOptions, SceneRendererError, Shader};
use egui_wgpu::wgpu;

pub struct GridRenderer {
    shader: Ref<Shader>,
    camera_bind_group: wgpu::BindGroup,
}

impl GridRenderer {
    pub fn new(
        context: &ReadOnlyAssetContext,
        camera_uniform_buffer: &wgpu::Buffer,
    ) -> Result<Self, SceneRendererError> {
        let shader = SceneRendererError::load::<Shader>(context, "shaders/grid")?;
        let camera_bind_group =
            context
                .render_context
                .device()
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("camera_bind_group"),
                    layout: &shader.read().bind_group_layouts[0],
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: camera_uniform_buffer.as_entire_binding(),
                    }],
                });
        Ok(Self {
            shader,
            camera_bind_group,
        })
    }

    pub fn camera_bind_group(&self) -> &wgpu::BindGroup {
        &self.camera_bind_group
    }

    pub fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        scene_texture_msaa: &Texture,
        scene_depth_texture: &Texture,
        samples: u32,
    ) {
        let shader = self.shader.read();
        let mut render_pass = RenderUtils::overlay_pass(
            encoder,
            &scene_texture_msaa.view,
            Some(&scene_depth_texture.view),
            "Scene Grid",
        );

        let options = PipelineOptions {
            samples,
            depth_stencil: Some(wgpu::DepthStencilState {
                depth_write_enabled: false,
                ..RenderUtils::depth_default(wgpu::TextureFormat::Depth32Float)
            }),
            fragment_targets: vec![Some(RenderUtils::color_alpha_blending(
                scene_texture_msaa.descriptor.format,
            ))],
            ..PipelineOptions::fullscreen()
        };
        render_pass.set_pipeline(&shader.pipeline(&options));
        render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
        RenderUtils::draw_fullscreen(&mut render_pass);
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

        let renderer = GridRenderer::new(&read_only_context, &camera_uniform_buffer)
            .expect("grid renderer should load its shader");

        let _ = renderer.camera_bind_group();
    }
}
