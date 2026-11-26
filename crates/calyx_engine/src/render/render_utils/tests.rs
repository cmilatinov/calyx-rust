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
