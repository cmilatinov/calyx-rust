use crate::assets::mesh::{Instance, Mesh};
use crate::assets::texture::Texture;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::render_utils::RenderUtils;
use crate::render::{PipelineOptions, Shader};
use egui::Color32;
use egui_wgpu::{wgpu, RenderState};
use nalgebra_glm::Mat4;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct OutlineUniform {
    selected_object_id: u32,
    hovered_object_id: u32,
    _padding: [u32; 2],
    selected_color: [f32; 4],
    hovered_color: [f32; 4],
}

pub struct OutlineRenderer {
    shader: Ref<Shader>,
    outline_uniform_buffer: wgpu::Buffer,
    selected_object_id: u32,
    hovered_object_id: u32,
    selected_color: Color32,
    hovered_color: Color32,
}

impl OutlineRenderer {
    pub fn new(context: &ReadOnlyAssetContext, device: &wgpu::Device) -> Self {
        let shader = context
            .registries
            .assets
            .read()
            .load::<Shader>("shaders/object_outline")
            .expect("missing object_outline shader");
        let outline_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("outline_uniform_buffer"),
            size: size_of::<OutlineUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            shader,
            outline_uniform_buffer,
            selected_object_id: 0,
            hovered_object_id: 0,
            selected_color: Color32::from_rgb(255, 149, 0),
            hovered_color: Color32::from_rgb(72, 184, 255),
        }
    }

    pub fn set_selected_object_id(&mut self, selected_object_id: u32) {
        self.selected_object_id = selected_object_id;
    }

    pub fn set_hovered_object_id(&mut self, hovered_object_id: u32) {
        self.hovered_object_id = hovered_object_id;
    }

    pub fn render(
        &mut self,
        render_state: &RenderState,
        encoder: &mut wgpu::CommandEncoder,
        screen_space_quad: &Ref<Mesh>,
        scene_texture_msaa: &Texture,
        scene_object_id_texture: &Texture,
        samples: u32,
    ) {
        if self.selected_object_id == 0 && self.hovered_object_id == 0 {
            return;
        }

        let device = &render_state.device;
        let queue = &render_state.queue;
        let mut quad_mesh = screen_space_quad.write();
        let mut shader = self.shader.write();
        let uniform = OutlineUniform {
            selected_object_id: self.selected_object_id,
            hovered_object_id: self.hovered_object_id,
            _padding: Default::default(),
            selected_color: color32_to_linear(self.selected_color),
            hovered_color: color32_to_linear(self.hovered_color),
        };
        queue.write_buffer(
            &self.outline_uniform_buffer,
            0,
            bytemuck::bytes_of(&uniform),
        );

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("outline_bind_group"),
            layout: &shader.bind_group_layouts[0],
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&scene_object_id_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.outline_uniform_buffer.as_entire_binding(),
                },
            ],
        });

        let options = PipelineOptions::builder()
            .samples(samples)
            .fragment_targets(vec![Some(RenderUtils::color_alpha_blending(
                scene_texture_msaa.descriptor.format,
            ))])
            .depth_stencil(None)
            .build();
        shader.build_pipeline(&options);
        let Some(pipeline) = shader.get_pipeline(&options) else {
            return;
        };

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Scene Outline"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &scene_texture_msaa.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, &bind_group, &[]);
        quad_mesh.instances.resize(
            1,
            Instance {
                bone_transform_index: -1,
                object_id: 0,
                _padding: Default::default(),
                transform: Mat4::identity().into(),
            },
        );
        RenderUtils::render_mesh(device, queue, &mut render_pass, &mut quad_mesh);
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
    use crate::test_utils::test_asset_context_with_assets;
    use egui_wgpu::wgpu::TextureFormat;
    use std::path::PathBuf;

    fn assets_path() -> PathBuf {
        let assets_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets");
        dunce::canonicalize(assets_path).expect("assets dir not found")
    }

    #[test]
    fn outline_renderer_loads_shader_and_builds_pipeline() {
        let context = test_asset_context_with_assets(vec![assets_path()]);
        let read_only_context = context.lock_read();
        let device = read_only_context.render_context.device();
        let renderer = OutlineRenderer::new(&read_only_context, device);
        let mut shader = renderer.shader.write();
        let options = crate::render::PipelineOptions::builder()
            .samples(1)
            .fragment_targets(vec![Some(RenderUtils::color_alpha_blending(
                TextureFormat::Rgba16Float,
            ))])
            .depth_stencil(None)
            .build();
        shader.build_pipeline(&options);
        assert!(shader.get_pipeline(&options).is_some());
    }
}
