use std::ops::Range;

use egui::Color32;
use egui_wgpu::wgpu;

use crate::assets::mesh::Mesh;

/// Small helpers for common render-pass and mesh-submission operations.
pub struct RenderUtils;

impl RenderUtils {
    /// Builds a color attachment that clears to `clear_color`.
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

    /// Builds a depth/stencil attachment that clears depth to `clear_value`.
    pub fn depth_stencil_attachment(
        view: &wgpu::TextureView,
        clear_value: f32,
        stencil_ops: Option<wgpu::Operations<u32>>,
    ) -> wgpu::RenderPassDepthStencilAttachment<'_> {
        wgpu::RenderPassDepthStencilAttachment {
            view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(clear_value),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops,
        }
    }

    /// Begins a pass that draws over what `color` and `depth` already hold.
    ///
    /// Both attachments load their contents and store the result, with no
    /// resolve target. Pass `None` for `depth` when the overlay ignores depth.
    pub fn overlay_pass<'e>(
        encoder: &'e mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: Option<&wgpu::TextureView>,
        label: &str,
    ) -> wgpu::RenderPass<'e> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: depth.map(|view| wgpu::RenderPassDepthStencilAttachment {
                view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        })
    }

    /// Begins a pass that overwrites every pixel of `output`, with no depth.
    ///
    /// It clears rather than loads, since a full-screen draw never reads the
    /// old contents.
    pub fn fullscreen_pass<'e>(
        encoder: &'e mut wgpu::CommandEncoder,
        output: &wgpu::TextureView,
        label: &str,
    ) -> wgpu::RenderPass<'e> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(Self::color_attachment(output, Color32::TRANSPARENT))],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        })
    }

    /// Draws the full-screen triangle of `shaders/fullscreen.wgsl`:
    /// three vertices, no vertex buffers, one instance. The pipeline must
    /// come from [`crate::render::PipelineOptions::fullscreen_builder`].
    pub fn draw_fullscreen(render_pass: &mut wgpu::RenderPass<'_>) {
        render_pass.draw(0..3, 0..1);
    }

    /// Returns a color target state configured for alpha blending.
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

    /// Returns a color target state configured for additive blending.
    pub fn color_additive_blending(format: wgpu::TextureFormat) -> wgpu::ColorTargetState {
        wgpu::ColorTargetState {
            format,
            blend: Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::SrcAlpha,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::OVER,
            }),
            write_mask: wgpu::ColorWrites::ALL,
        }
    }

    /// Returns a default color target state for `texture_format`.
    pub fn color_default(texture_format: wgpu::TextureFormat) -> wgpu::ColorTargetState {
        texture_format.into()
    }

    /// Returns a default depth state for `format`.
    pub fn depth_default(format: wgpu::TextureFormat) -> wgpu::DepthStencilState {
        wgpu::DepthStencilState {
            format,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }
    }

    /// Returns a default multisample state for `samples`.
    pub fn multisample_default(samples: u32) -> wgpu::MultisampleState {
        wgpu::MultisampleState {
            count: samples,
            ..Default::default()
        }
    }

    /// Rebuilds mesh CPU/GPU data as needed before drawing.
    pub fn rebuild_mesh_data(device: &wgpu::Device, queue: &wgpu::Queue, mesh: &mut Mesh) {
        if mesh.dirty {
            mesh.rebuild_mesh_data(device);
            mesh.dirty = false;
        }
        mesh.rebuild_instance_data(device, queue);
    }

    /// Binds the mesh vertex and index buffers on `render_pass`.
    pub fn bind_mesh_buffers(render_pass: &mut wgpu::RenderPass<'_>, mesh: &Mesh) {
        if !mesh.indices.is_empty() {
            render_pass.set_index_buffer(
                mesh.index_buffer.as_ref().unwrap().slice(..),
                wgpu::IndexFormat::Uint32,
            );
        }
        render_pass.set_vertex_buffer(0, mesh.vertex_buffer.as_ref().unwrap().slice(..));
    }

    /// Issues an indexed instanced draw for `mesh`.
    pub fn draw_mesh_instanced(
        render_pass: &mut wgpu::RenderPass<'_>,
        mesh: &Mesh,
        instances: Range<u32>,
    ) {
        render_pass.draw_indexed(0..(mesh.indices.len() as u32), 0, instances);
    }

    /// Rebuilds buffers if needed and renders the mesh once.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_render_context;

    const SIZE: u32 = 4;

    fn target(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("overlay_pass_test_target"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        (texture, view)
    }

    /// Copies the first texel of each row of `texture` into a buffer and reads it back.
    fn read_texels(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mut encoder: wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        aspect: wgpu::TextureAspect,
    ) -> Vec<u8> {
        let bytes_per_row = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay_pass_test_readback"),
            size: (bytes_per_row * SIZE) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: Default::default(),
                aspect,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(SIZE),
                },
            },
            texture.size(),
        );
        queue.submit(Some(encoder.finish()));
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |result| result.expect("map failed"));
        let _ = device.poll(wgpu::Maintain::Wait);
        let bytes = slice.get_mapped_range().to_vec();
        buffer.unmap();
        bytes
    }

    #[test]
    fn overlay_pass_keeps_existing_color_and_depth() {
        let render_context = test_render_context();
        let device = render_context.device();
        let queue = render_context.queue();
        let (color, color_view) = target(device, wgpu::TextureFormat::Rgba8Unorm);
        let (depth, depth_view) = target(device, wgpu::TextureFormat::Depth32Float);

        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("overlay_pass_test_clear"),
            color_attachments: &[Some(RenderUtils::color_attachment(
                &color_view,
                Color32::RED,
            ))],
            depth_stencil_attachment: Some(RenderUtils::depth_stencil_attachment(
                &depth_view,
                0.25,
                None,
            )),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        RenderUtils::overlay_pass(&mut encoder, &color_view, Some(&depth_view), "overlay");
        queue.submit(Some(encoder.finish()));

        let encoder = device.create_command_encoder(&Default::default());
        let colors = read_texels(device, queue, encoder, &color, wgpu::TextureAspect::All);
        let encoder = device.create_command_encoder(&Default::default());
        let depths = read_texels(
            device,
            queue,
            encoder,
            &depth,
            wgpu::TextureAspect::DepthOnly,
        );

        assert_eq!(
            &colors[..4],
            &[255, 0, 0, 255],
            "color must be loaded and stored"
        );
        let depth_value = f32::from_le_bytes(depths[..4].try_into().unwrap());
        assert_eq!(depth_value, 0.25, "depth must be loaded and stored");
    }

    #[test]
    fn overlay_pass_runs_without_a_depth_attachment() {
        let render_context = test_render_context();
        let device = render_context.device();
        let queue = render_context.queue();
        let (color, color_view) = target(device, wgpu::TextureFormat::Rgba8Unorm);

        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("overlay_pass_test_clear"),
            color_attachments: &[Some(RenderUtils::color_attachment(
                &color_view,
                Color32::BLUE,
            ))],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        RenderUtils::overlay_pass(&mut encoder, &color_view, None, "overlay");
        let colors = read_texels(device, queue, encoder, &color, wgpu::TextureAspect::All);

        assert_eq!(
            &colors[..4],
            &[0, 0, 255, 255],
            "color must be loaded and stored"
        );
    }
}
