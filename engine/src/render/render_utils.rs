use std::ops::Range;

use egui::Color32;
use egui_wgpu::wgpu;

use crate::assets::mesh::Mesh;

pub struct RenderUtils;

impl RenderUtils {
    pub fn color_attachment(
        view: &wgpu::TextureView,
        clear_color: Color32,
    ) -> wgpu::RenderPassColorAttachment<'_> {
        let [x, y, z, w] = clear_color.to_array();
        wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: x as f64 / 255.0,
                    g: y as f64 / 255.0,
                    b: z as f64 / 255.0,
                    a: w as f64 / 255.0,
                }),
                store: wgpu::StoreOp::Store,
            },
        }
    }

    pub fn depth_stencil_attachment(
        view: &wgpu::TextureView,
        clear_value: f32,
        stencil_ops: Option<wgpu::Operations<u32>>,
    ) -> wgpu::RenderPassDepthStencilAttachment {
        wgpu::RenderPassDepthStencilAttachment {
            view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(clear_value),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops,
        }
    }

    pub fn color_alpha_blending(format: wgpu::TextureFormat) -> wgpu::ColorTargetState {
        wgpu::ColorTargetState {
            format,
            blend: Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::SrcAlpha,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::OVER,
            }),
            write_mask: wgpu::ColorWrites::ALL,
        }
    }

    pub fn color_default(texture_format: wgpu::TextureFormat) -> wgpu::ColorTargetState {
        texture_format.into()
    }

    pub fn depth_default(format: wgpu::TextureFormat) -> wgpu::DepthStencilState {
        wgpu::DepthStencilState {
            format,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }
    }

    pub fn multisample_default(samples: u32) -> wgpu::MultisampleState {
        wgpu::MultisampleState {
            count: samples,
            ..Default::default()
        }
    }

    pub fn rebuild_mesh_data(device: &wgpu::Device, queue: &wgpu::Queue, mesh: &mut Mesh) {
        if mesh.dirty {
            mesh.rebuild_mesh_data(device);
            mesh.dirty = false;
        }
        mesh.rebuild_instance_data(device, queue);
    }

    pub fn bind_mesh_buffers<'a>(render_pass: &mut wgpu::RenderPass<'a>, mesh: &'a Mesh) {
        if !mesh.indices.is_empty() {
            render_pass.set_index_buffer(
                mesh.index_buffer.as_ref().unwrap().slice(..),
                wgpu::IndexFormat::Uint32,
            );
        }
        render_pass.set_vertex_buffer(0, mesh.vertex_buffer.as_ref().unwrap().slice(..));
    }

    pub fn draw_mesh_instanced<'a>(
        render_pass: &mut wgpu::RenderPass<'a>,
        mesh: &'a Mesh,
        instances: Range<u32>,
    ) {
        render_pass.draw_indexed(0..(mesh.indices.len() as u32), 0, instances);
    }

    pub fn render_mesh<'a>(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        render_pass: &mut wgpu::RenderPass<'a>,
        mesh: &'a mut Mesh,
    ) {
        Self::rebuild_mesh_data(device, queue, mesh);
        Self::bind_mesh_buffers(render_pass, mesh);
        Self::draw_mesh_instanced(render_pass, mesh, 0..(mesh.instances.len() as u32));
    }
}
