use crate::assets::texture::Texture;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::render_utils::RenderUtils;
use crate::render::{CachedBindGroup, PipelineOptions, RenderContext, SceneRendererError, Shader};
use egui::Color32;
use egui_wgpu::wgpu;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct OutlineUniform {
    target_object_id: u32,
    outline_radius: u32,
    _padding: [u32; 2],
    outline_color: [f32; 4],
}

/// One outline pass's uniforms and bind group. Each pass has its own uniform
/// buffer: both passes are recorded before one submit, so a shared buffer
/// would hold only the last pass's values.
pub(crate) struct OutlinePass {
    uniform_buffer: wgpu::Buffer,
    bind_group: CachedBindGroup<(wgpu::BindGroupLayout, wgpu::TextureView)>,
}

impl OutlinePass {
    #[cfg(test)]
    pub(crate) fn is_cached(&self) -> bool {
        self.bind_group.is_cached()
    }

    fn new(device: &wgpu::Device) -> Self {
        Self {
            uniform_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("outline_uniform_buffer"),
                size: size_of::<OutlineUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            bind_group: Default::default(),
        }
    }
}

pub struct OutlineRenderer {
    shader: Ref<Shader>,
    pub(crate) hovered_pass: OutlinePass,
    pub(crate) selected_pass: OutlinePass,
    selected_object_id: u32,
    hovered_object_id: u32,
    selected_color: Color32,
    hovered_color: Color32,
}

impl OutlineRenderer {
    const SELECTED_OUTLINE_RADIUS: u32 = 1;
    const HOVERED_OUTLINE_RADIUS: u32 = 2;

    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        let shader = SceneRendererError::load::<Shader>(context, "shaders/object_outline")?;
        let device = context.render_context.device();
        Ok(Self {
            shader,
            hovered_pass: OutlinePass::new(device),
            selected_pass: OutlinePass::new(device),
            selected_object_id: 0,
            hovered_object_id: 0,
            selected_color: Color32::from_rgb(255, 149, 0),
            hovered_color: Color32::from_rgba_unmultiplied(143, 214, 255, 220),
        })
    }

    pub fn set_selected_object_id(&mut self, selected_object_id: u32) {
        self.selected_object_id = selected_object_id;
    }

    pub fn set_hovered_object_id(&mut self, hovered_object_id: u32) {
        self.hovered_object_id = hovered_object_id;
    }

    pub fn render(
        &mut self,
        render_context: &RenderContext,
        encoder: &mut wgpu::CommandEncoder,
        scene_texture_msaa: &Texture,
        scene_object_id_texture: &Texture,
        samples: u32,
    ) {
        // An inactive pass releases the object-id view it bound, which a
        // resize may have replaced since.
        if self.hovered_object_id == 0 {
            self.hovered_pass.bind_group.clear();
        }
        if self.selected_object_id == 0 {
            self.selected_pass.bind_group.clear();
        }
        if self.selected_object_id == 0 && self.hovered_object_id == 0 {
            return;
        }

        let shader = self.shader.read();
        let options = PipelineOptions {
            samples,
            fragment_targets: vec![Some(RenderUtils::color_alpha_blending(
                scene_texture_msaa.descriptor.format,
            ))],
            ..PipelineOptions::fullscreen()
        };
        let pipeline = shader.pipeline(&options);
        let bind_group_layout = shader.bind_group_layouts[0].clone();
        drop(shader);

        if self.hovered_object_id != 0 {
            Self::render_pass(
                &mut self.hovered_pass,
                render_context,
                encoder,
                &scene_texture_msaa.view,
                &scene_object_id_texture.view,
                &pipeline,
                &bind_group_layout,
                self.hovered_object_id,
                self.hovered_color,
                Self::HOVERED_OUTLINE_RADIUS,
            );
        }

        if self.selected_object_id != 0 {
            Self::render_pass(
                &mut self.selected_pass,
                render_context,
                encoder,
                &scene_texture_msaa.view,
                &scene_object_id_texture.view,
                &pipeline,
                &bind_group_layout,
                self.selected_object_id,
                self.selected_color,
                Self::SELECTED_OUTLINE_RADIUS,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_pass(
        pass: &mut OutlinePass,
        render_context: &RenderContext,
        encoder: &mut wgpu::CommandEncoder,
        scene_texture_view: &wgpu::TextureView,
        object_id_texture_view: &wgpu::TextureView,
        pipeline: &wgpu::RenderPipeline,
        bind_group_layout: &wgpu::BindGroupLayout,
        target_object_id: u32,
        outline_color: Color32,
        outline_radius: u32,
    ) {
        let device = render_context.device();
        let queue = render_context.queue();
        let uniform = OutlineUniform {
            target_object_id,
            outline_radius,
            _padding: Default::default(),
            outline_color: color32_to_linear(outline_color),
        };
        queue.write_buffer(&pass.uniform_buffer, 0, bytemuck::bytes_of(&uniform));

        let uniform_buffer = &pass.uniform_buffer;
        let bind_group = pass.bind_group.get_or_create(
            (bind_group_layout.clone(), object_id_texture_view.clone()),
            || {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("outline_bind_group"),
                    layout: bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(object_id_texture_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: uniform_buffer.as_entire_binding(),
                        },
                    ],
                })
            },
        );

        let mut render_pass =
            RenderUtils::overlay_pass(encoder, scene_texture_view, None, "Scene Outline");
        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        RenderUtils::draw_fullscreen(&mut render_pass);
    }
}

fn color32_to_linear(color: Color32) -> [f32; 4] {
    [
        srgb_channel_to_linear(color.r()),
        srgb_channel_to_linear(color.g()),
        srgb_channel_to_linear(color.b()),
        color.a() as f32 / 255.0,
    ]
}

fn srgb_channel_to_linear(channel: u8) -> f32 {
    let srgb = channel as f32 / 255.0;
    if srgb <= 0.04045 {
        srgb / 12.92
    } else {
        ((srgb + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::OutlineRenderer;
    use crate::render::RenderUtils;
    use crate::test_support::TestAssets;
    use egui_wgpu::wgpu::TextureFormat;

    #[test]
    fn outline_renderer_loads_shader_and_builds_pipeline() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let renderer =
            OutlineRenderer::new(&read_only_context).expect("outline renderer should load");
        let shader = renderer.shader.read();
        let options = crate::render::PipelineOptions {
            fragment_targets: vec![Some(RenderUtils::color_alpha_blending(
                TextureFormat::Rgba16Float,
            ))],
            ..crate::render::PipelineOptions::fullscreen()
        };
        let pipeline = shader.pipeline(&options);
        assert_eq!(shader.pipeline(&options), pipeline);
    }
}
