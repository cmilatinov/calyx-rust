use super::Texture;
use crate::test_support::TestAssets;
use egui_wgpu::wgpu;

const TOP: [u8; 4] = [255, 0, 0, 255];
const BOTTOM: [u8; 4] = [0, 0, 255, 255];

/// Reads the first texel of each row of `mip` of `texture`, an 8-bit RGBA texture of
/// `owner`'s size: its stored texture or its sRGB copy.
fn read_mip_rows(owner: &Texture, texture: &wgpu::Texture, mip: u32) -> Vec<[u8; 4]> {
    let device = owner.render_context.device();
    let queue = owner.render_context.queue();
    let size = owner
        .descriptor
        .size
        .mip_level_size(mip, owner.descriptor.dimension);
    let bytes_per_row = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("mip_test_readback"),
        size: (bytes_per_row * size.height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: mip,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(size.height),
            },
        },
        size,
    );
    queue.submit(Some(encoder.finish()));
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| result.expect("map failed"));
    let _ = device.poll(wgpu::Maintain::Wait);
    let bytes = slice.get_mapped_range().to_vec();
    buffer.unmap();
    (0..size.height as usize)
        .map(|row| {
            let start = row * bytes_per_row as usize;
            bytes[start..start + 4].try_into().unwrap()
        })
        .collect()
}

#[test]
fn a_writable_texture_starts_filled_and_takes_writes_in_place() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let white = [255; 4];
    let texture =
        Texture::writable_rgba8(context.render_context.clone(), "writable_test", 4, 4, white);
    assert_eq!(read_mip_rows(&texture, &texture.texture, 0), vec![white; 4]);

    texture.write_rgba8(0, 1, 1, 2, &[TOP, BOTTOM].concat());
    assert_eq!(
        read_mip_rows(&texture, &texture.texture, 0),
        vec![white, TOP, BOTTOM, white],
        "only the written rectangle changes"
    );
}

#[test]
fn generated_2d_mips_keep_the_image_upright() {
    const SIZE: u32 = 8;
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let extent = wgpu::Extent3d {
        width: SIZE,
        height: SIZE,
        depth_or_array_layers: 1,
    };
    let texture = Texture::new(
        context.render_context.clone(),
        &wgpu::TextureDescriptor {
            label: Some("mip_test_texture"),
            size: extent,
            mip_level_count: Texture::mip_level_count(SIZE, SIZE),
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        },
        None,
        None,
        false,
    );
    // Top half one color, bottom half another.
    let texels = (0..SIZE * SIZE)
        .flat_map(|index| if index / SIZE < SIZE / 2 { TOP } else { BOTTOM })
        .collect::<Vec<_>>();
    context.render_context.queue().write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture.texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &texels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * SIZE),
            rows_per_image: Some(SIZE),
        },
        extent,
    );

    texture
        .generate_2d_mips(&context)
        .expect("mip generation runs");

    let rows = read_mip_rows(&texture, &texture.texture, 1);
    assert_eq!(rows.first(), Some(&TOP), "mip 1's top row must stay on top");
    assert_eq!(
        rows.last(),
        Some(&BOTTOM),
        "mip 1's bottom row must stay below"
    );
}

#[test]
fn an_image_s_srgb_mips_average_in_linear_light_and_its_stored_mips_average_as_stored() {
    const SIZE: u32 = 4;
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    // A black and white checkerboard: every 2x2 block of mip 0 averages to half.
    let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(SIZE, SIZE, |x, y| {
        let value = if (x + y) % 2 == 0 { 255 } else { 0 };
        image::Rgba([value, value, value, 255])
    }));

    let texture = Texture::from_image(&context, "test/checker", image).expect("the image uploads");

    let srgb_texture = texture
        .srgb_texture
        .as_ref()
        .expect("an 8-bit image has an sRGB copy");
    // Linear 0.5 encodes to sRGB 188; averaging the encoded channels gives 128.
    for [r, g, b, a] in read_mip_rows(&texture, srgb_texture, 1) {
        for channel in [r, g, b] {
            assert!(
                channel.abs_diff(188) <= 2,
                "the sRGB mip holds linear half, got {channel}"
            );
        }
        assert_eq!(a, 255);
    }
    for [r, g, b, _] in read_mip_rows(&texture, &texture.texture, 1) {
        for channel in [r, g, b] {
            assert!(
                channel.abs_diff(128) <= 2,
                "the stored mip holds the stored half, got {channel}"
            );
        }
    }
}

#[test]
fn imported_samplers_keep_their_wrap_modes_and_filters() {
    use crate::assets::import::{ImportedSampler, TextureFilter, TextureWrap};

    let repeating = Texture::sampler_descriptor("repeat", &ImportedSampler::default());
    assert_eq!(repeating.address_mode_u, wgpu::AddressMode::Repeat);
    assert_eq!(repeating.address_mode_v, wgpu::AddressMode::Repeat);
    assert_eq!(repeating.min_filter, wgpu::FilterMode::Linear);
    assert_eq!(repeating.mipmap_filter, wgpu::FilterMode::Linear);
    assert!(repeating.lod_max_clamp > 0.0);

    let unmipped = Texture::sampler_descriptor(
        "nearest",
        &ImportedSampler {
            wrap_u: TextureWrap::MirroredRepeat,
            wrap_v: TextureWrap::ClampToEdge,
            mag_filter: TextureFilter::Nearest,
            min_filter: TextureFilter::Nearest,
            mipmap_filter: None,
        },
    );
    assert_eq!(unmipped.address_mode_u, wgpu::AddressMode::MirrorRepeat);
    assert_eq!(unmipped.address_mode_v, wgpu::AddressMode::ClampToEdge);
    assert_eq!(unmipped.mag_filter, wgpu::FilterMode::Nearest);
    assert_eq!(unmipped.min_filter, wgpu::FilterMode::Nearest);
    assert_eq!(
        unmipped.lod_max_clamp, 0.0,
        "only the full-size image is sampled"
    );
}

#[test]
fn mip_level_count_includes_base_level() {
    assert_eq!(Texture::mip_level_count(1, 1), 1);
    assert_eq!(Texture::mip_level_count(2, 2), 2);
    assert_eq!(Texture::mip_level_count(4, 4), 3);
}

#[test]
fn mip_level_count_uses_largest_dimension() {
    assert_eq!(Texture::mip_level_count(8, 4), 4);
    assert_eq!(Texture::mip_level_count(4, 8), 4);
    assert_eq!(Texture::mip_level_count(7, 3), 3);
}
