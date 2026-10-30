use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::overlay::{OverlayContext, OverlayPass};
use crate::render::render_utils::RenderUtils;
use crate::render::{CachedBindGroup, PipelineOptions, SceneRendererError, Shader};
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

/// One outline's uniforms and bind group. Each outline has its own uniform
/// buffer: both draw in one render pass before one submit, so a shared
/// buffer would hold only the last outline's values.
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
        let shader = SceneRendererError::load::<Shader>(context, "shaders/editor/object_outline")?;
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

    /// Sets the object outlined as selected; `0` outlines nothing.
    pub fn set_selected_object_id(&mut self, selected_object_id: u32) {
        self.selected_object_id = selected_object_id;
        // An inactive pass releases the object-id view it bound, which a
        // resize may have replaced since.
        if selected_object_id == 0 {
            self.selected_pass.bind_group.clear();
        }
    }

    /// Sets the object outlined as hovered; `0` outlines nothing.
    pub fn set_hovered_object_id(&mut self, hovered_object_id: u32) {
        self.hovered_object_id = hovered_object_id;
        if hovered_object_id == 0 {
            self.hovered_pass.bind_group.clear();
        }
    }

    /// The outline passes that draw this frame, hovered first, each with
    /// the object it outlines, its color and its radius.
    fn active_passes(&mut self) -> impl Iterator<Item = (&mut OutlinePass, u32, Color32, u32)> {
        [
            (
                &mut self.hovered_pass,
                self.hovered_object_id,
                self.hovered_color,
                Self::HOVERED_OUTLINE_RADIUS,
            ),
            (
                &mut self.selected_pass,
                self.selected_object_id,
                self.selected_color,
                Self::SELECTED_OUTLINE_RADIUS,
            ),
        ]
        .into_iter()
        .filter(|(_, object_id, _, _)| *object_id != 0)
    }
}

/// Outlines the hovered and the selected object from the object-id texture,
/// over the post-processed scene. Each outline is one full-screen draw.
impl OverlayPass for OutlineRenderer {
    fn name(&self) -> &str {
        "outline"
    }

    fn enabled(&self, _context: &OverlayContext) -> bool {
        self.selected_object_id != 0 || self.hovered_object_id != 0
    }

    fn prepare(&mut self, context: &OverlayContext) {
        let layout = self.shader.read().bind_group_layouts[0].clone();
        let device = context.render_context.device();
        let queue = context.render_context.queue();
        let object_ids = &context.object_ids.view;
        for (pass, target_object_id, outline_color, outline_radius) in self.active_passes() {
            let uniform = OutlineUniform {
                target_object_id,
                outline_radius,
                _padding: Default::default(),
                outline_color: color32_to_linear(outline_color),
            };
            queue.write_buffer(&pass.uniform_buffer, 0, bytemuck::bytes_of(&uniform));
            let uniform_buffer = &pass.uniform_buffer;
            pass.bind_group
                .get_or_create((layout.clone(), object_ids.clone()), || {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("outline_bind_group"),
                        layout: &layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(object_ids),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: uniform_buffer.as_entire_binding(),
                            },
                        ],
                    })
                });
        }
    }

    fn record(&mut self, context: &OverlayContext, render_pass: &mut wgpu::RenderPass<'_>) {
        let pipeline = self
            .shader
            .read()
            .pipeline(&Self::pipeline_options(context));
        render_pass.set_pipeline(&pipeline);
        for (pass, ..) in self.active_passes() {
            // `prepare` built the group for this frame's object-id view.
            let Some(bind_group) = pass.bind_group.get() else {
                continue;
            };
            render_pass.set_bind_group(0, bind_group, &[]);
            RenderUtils::draw_fullscreen(render_pass);
        }
    }
}

impl OutlineRenderer {
    /// The outline's pipeline: a full-screen draw blended into the overlay
    /// target, which ignores the overlay pass's depth.
    fn pipeline_options(context: &OverlayContext) -> PipelineOptions {
        PipelineOptions::fullscreen_builder()
            .samples(context.samples)
            .target(RenderUtils::color_alpha_blending(context.format))
            .depth_stencil(wgpu::DepthStencilState {
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                ..RenderUtils::depth_default(wgpu::TextureFormat::Depth32Float)
            })
            .build()
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
        let options = crate::render::PipelineOptions::fullscreen_builder()
            .target(RenderUtils::color_alpha_blending(
                TextureFormat::Rgba16Float,
            ))
            .build();
        let pipeline = shader.pipeline(&options);
        assert_eq!(shader.pipeline(&options), pipeline);
    }
}
