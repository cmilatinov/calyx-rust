use std::ops::Range;

use egui_wgpu::wgpu;
use glm::Vec4;

use crate::assets::mesh::Mesh;

pub struct RenderUtils;

impl RenderUtils {
    pub fn color_attachment<'a>(
        view: &'a wgpu::TextureView,
        clear_color: &Vec4,
    ) -> wgpu::RenderPassColorAttachment<'a> {
        wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: clear_color.x as f64,
                    g: clear_color.y as f64,
                    b: clear_color.z as f64,
                    a: clear_color.w as f64,
                }),
                store: true,
            },
        }
    }

    pub fn depth_stencil_attachment(
        view: &wgpu::TextureView,
        clear_value: f32,
    ) -> wgpu::RenderPassDepthStencilAttachment {
        wgpu::RenderPassDepthStencilAttachment {
            view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(clear_value),
                store: true,
            }),
            stencil_ops: None,
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

    pub fn primitive_default(topology: wgpu::PrimitiveTopology) -> wgpu::PrimitiveState {
        wgpu::PrimitiveState {
            topology,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        }
    }

    pub fn depth_default() -> wgpu::DepthStencilState {
        wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
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
        render_pass.set_index_buffer(
            mesh.index_buffer.as_ref().unwrap().slice(..),
            wgpu::IndexFormat::Uint32,
        );
        render_pass.set_vertex_buffer(0, mesh.vertex_buffer.as_ref().unwrap().slice(..));
        render_pass.set_vertex_buffer(1, mesh.instance_buffer.get_wgpu_buffer().slice(..));
    }

    pub fn render_mesh<'a>(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        render_pass: &mut wgpu::RenderPass<'a>,
        mesh: &'a mut Mesh,
    ) {
        Self::rebuild_mesh_data(device, queue, mesh);
        Self::bind_mesh_buffers(render_pass, mesh);
        render_pass.draw_indexed(
            0..(mesh.indices.len() as u32),
            0,
            0..(mesh.instances.len() as u32),
        );
    }

    pub fn render_mesh_instanced<'a>(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        render_pass: &mut wgpu::RenderPass<'a>,
        mesh: &'a mut Mesh,
        instances: Range<u32>,
    ) {
        Self::rebuild_mesh_data(device, queue, mesh);
        render_pass.set_index_buffer(
            mesh.index_buffer.as_ref().unwrap().slice(..),
            wgpu::IndexFormat::Uint32,
        );
        render_pass.set_vertex_buffer(0, mesh.vertex_buffer.as_ref().unwrap().slice(..));
        render_pass.draw_indexed(0..(mesh.indices.len() as u32), 0, instances);
    }
}
