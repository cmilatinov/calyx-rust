use crate::assets::mesh::{Instance, Mesh};
use crate::assets::texture::Texture;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::render_utils::RenderUtils;
use crate::render::{PipelineOptions, Shader};
use egui_wgpu::{wgpu, RenderState};
use nalgebra_glm::Mat4;

pub struct GridRenderer {
    shader: Ref<Shader>,
    camera_bind_group: wgpu::BindGroup,
}

impl GridRenderer {
    pub fn new(
        context: &ReadOnlyAssetContext,
        device: &wgpu::Device,
        camera_uniform_buffer: &wgpu::Buffer,
    ) -> Self {
        let shader = context
            .registries
            .assets
            .read()
            .load::<Shader>("shaders/grid")
            .expect("missing grid_shader");
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera_bind_group"),
            layout: &shader.read().bind_group_layouts[0],
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_uniform_buffer.as_entire_binding(),
            }],
        });
        Self {
            shader,
            camera_bind_group,
        }
    }

    pub fn camera_bind_group(&self) -> &wgpu::BindGroup {
        &self.camera_bind_group
    }

    pub fn render(
        &mut self,
        render_state: &RenderState,
        encoder: &mut wgpu::CommandEncoder,
        screen_space_quad: &Ref<Mesh>,
        scene_texture_msaa: &Texture,
        scene_depth_texture: &Texture,
        samples: u32,
    ) {
        let device = &render_state.device;
        let queue = &render_state.queue;
        let mut quad_mesh = screen_space_quad.write();
        let mut shader = self.shader.write();
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Scene Grid"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &scene_texture_msaa.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &scene_depth_texture.view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        let options = PipelineOptions::builder()
            .samples(samples)
            .fragment_targets(vec![Some(RenderUtils::color_alpha_blending(
                scene_texture_msaa.descriptor.format,
            ))])
            .build();
        shader.build_pipeline(&options);
        if let Some(pipeline) = shader.get_pipeline(&options) {
            render_pass.set_pipeline(pipeline);
            render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
            quad_mesh.instances.resize(
                1,
                Instance {
                    bone_transform_index: -1,
                    _padding: Default::default(),
                    transform: Mat4::identity().into(),
                },
            );
            RenderUtils::render_mesh(device, queue, &mut render_pass, &mut quad_mesh);
        }
    }
}
