use std::path::Path;

use egui_wgpu::wgpu;

use crate::assets::error::AssetError;
use crate::assets::Asset;
use crate::render::RenderContext;

pub struct Texture2D {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub handle: egui::TextureHandle,
    pub size: wgpu::Extent3d,
}

impl Asset for Texture2D {
    fn get_file_extensions() -> &'static [&'static str] {
        &["png", "jpg", "jpeg", "webp"]
    }

    fn from_file(path: &Path) -> Result<Self, AssetError> {
        let reader = image::io::Reader::open(path).map_err(|_| AssetError::LoadError)?;
        let mut texture_data = reader
            .decode()
            .map_err(|_| AssetError::LoadError)?
            .to_rgba8();
        for pixel in texture_data.pixels_mut() {
            if pixel[3] == 0 {
                pixel[0] = 0;
                pixel[1] = 0;
                pixel[2] = 0;
            }
        }
        let queue = RenderContext::queue().unwrap();
        let device = RenderContext::device().unwrap();
        let renderer = RenderContext::renderer().unwrap();
        let tex_mgr = RenderContext::texture_manager().unwrap();
        let texture_name = path.file_name().unwrap().to_str().unwrap();
        let texture_size = wgpu::Extent3d {
            width: texture_data.width(),
            height: texture_data.height(),
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(texture_name),
            size: texture_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some(texture_name),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let texture_id =
            renderer
                .write()
                .register_native_texture(&device, &view, wgpu::FilterMode::Linear);
        let handle = egui::TextureHandle::new(tex_mgr, texture_id);
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &texture_data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * texture_data.width()),
                rows_per_image: Some(texture_data.height()),
            },
            texture_size,
        );
        Ok(Self {
            texture,
            view,
            sampler,
            handle,
            size: texture_size,
        })
    }
}
