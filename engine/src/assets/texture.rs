use std::path::Path;

use egui_wgpu::wgpu;

use crate as engine;
use crate::assets::error::AssetError;
use crate::assets::Asset;
use crate::render::RenderContext;
use crate::utils::TypeUuid;

#[derive(TypeUuid)]
#[uuid = "8ba4ccec-85ab-45f5-b4ee-2e803ef548a2"]
pub struct Texture2D {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub handle: Option<egui::TextureHandle>,
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
        let texture_name = path.file_name().unwrap().to_str().unwrap();
        let texture_size = wgpu::Extent3d {
            width: texture_data.width(),
            height: texture_data.height(),
            depth_or_array_layers: 1,
        };
        let texture = Self::new(
            &wgpu::TextureDescriptor {
                label: Some(texture_name),
                size: texture_size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            Some(wgpu::SamplerDescriptor {
                label: Some(texture_name),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            true
        );
        let queue = RenderContext::queue().unwrap();
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture.texture,
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
        Ok(texture)
    }
}

impl Texture2D {
    pub fn new(
        texture_desc: &wgpu::TextureDescriptor,
        sampler_desc: Option<wgpu::SamplerDescriptor>,
        create_handle: bool,
    ) -> Self {
        let device = RenderContext::device().unwrap();

        let sampler_desc = sampler_desc.unwrap_or(wgpu::SamplerDescriptor {
            label: None,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let texture = device.create_texture(texture_desc);
        let sampler = device.create_sampler(&sampler_desc);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let handle = if create_handle {
            let renderer = RenderContext::renderer().unwrap();
            let tex_mgr = RenderContext::texture_manager().unwrap();
            let texture_id =
                renderer
                    .write()
                    .register_native_texture(&device, &view, wgpu::FilterMode::Linear);
            Some(egui::TextureHandle::new(tex_mgr, texture_id))
        } else {
            None
        };
        Self {
            texture,
            view,
            sampler,
            handle,
            size: texture_desc.size,
        }
    }
}

impl Drop for Texture2D {
    fn drop(&mut self) {
        if let Some(handle) = &self.handle {
            let renderer = RenderContext::renderer().unwrap();
            renderer.write().free_texture(&handle.id());
        }
    }
}
