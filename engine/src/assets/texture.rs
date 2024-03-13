use std::path::Path;

use egui_wgpu::wgpu;
use image::{ColorType, DynamicImage};

use crate as engine;
use crate::assets::error::AssetError;
use crate::assets::Asset;
use crate::render::RenderContext;
use crate::utils::TypeUuid;

use super::LoadedAsset;

#[derive(TypeUuid)]
#[uuid = "8ba4ccec-85ab-45f5-b4ee-2e803ef548a2"]
pub struct Texture2D {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub handle: Option<egui::TextureHandle>,
    pub size: wgpu::Extent3d,
    pub format: wgpu::TextureFormat,
}

impl Asset for Texture2D {
    fn get_file_extensions() -> &'static [&'static str] {
        &["png", "jpg", "jpeg", "webp"]
    }

    fn from_file(path: &Path) -> Result<LoadedAsset<Self>, AssetError> {
        let reader = image::io::Reader::open(path).map_err(|_| AssetError::LoadError)?;
        let texture_data =
            Self::transform_texture(reader.decode().map_err(|_| AssetError::LoadError)?);
        let texture_depth = texture_data.color().bytes_per_pixel() as u32;
        let texture_format = Self::texture_format(texture_data.color());
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
                format: texture_format,
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
            None,
            texture_format == wgpu::TextureFormat::Rgba8Unorm,
        );
        let queue = RenderContext::queue().unwrap();
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            texture_data.as_bytes(),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(texture_depth * texture_data.width()),
                rows_per_image: Some(texture_data.height()),
            },
            texture_size,
        );
        Ok(LoadedAsset::new(texture))
    }
}

impl Texture2D {
    pub fn new(
        texture_desc: &wgpu::TextureDescriptor,
        sampler_desc: Option<wgpu::SamplerDescriptor>,
        view_desc: Option<wgpu::TextureViewDescriptor>,
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
        let view_desc = view_desc.unwrap_or(Default::default());
        let texture = device.create_texture(texture_desc);
        let sampler = device.create_sampler(&sampler_desc);
        let view = texture.create_view(&view_desc);
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
            format: texture_desc.format,
        }
    }

    fn texture_format(color: ColorType) -> wgpu::TextureFormat {
        match color {
            ColorType::Rgba32F | ColorType::Rgb32F => wgpu::TextureFormat::Rgba32Float,
            ColorType::Rgba8 | ColorType::Rgb8 => wgpu::TextureFormat::Rgba8Unorm,
            _ => wgpu::TextureFormat::Rgba8Unorm,
        }
    }

    fn transform_texture(texture_data: DynamicImage) -> DynamicImage {
        match texture_data.color() {
            ColorType::Rgb8 => texture_data.to_rgba8().into(),
            ColorType::Rgb32F => texture_data.to_rgba32f().into(),
            _ => texture_data,
        }
    }

    pub fn create_cubemap_views(&self, mip_level: Option<u32>) -> [wgpu::TextureView; 6] {
        let base_mip_level = mip_level.unwrap_or_default();
        [
            self.texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: 0,
                array_layer_count: Some(1),
                base_mip_level,
                mip_level_count: Some(1),
                ..Default::default()
            }),
            self.texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: 1,
                array_layer_count: Some(1),
                base_mip_level,
                mip_level_count: Some(1),
                ..Default::default()
            }),
            self.texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: 2,
                array_layer_count: Some(1),
                base_mip_level,
                mip_level_count: Some(1),
                ..Default::default()
            }),
            self.texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: 3,
                array_layer_count: Some(1),
                base_mip_level,
                mip_level_count: Some(1),
                ..Default::default()
            }),
            self.texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: 4,
                array_layer_count: Some(1),
                base_mip_level,
                mip_level_count: Some(1),
                ..Default::default()
            }),
            self.texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: 5,
                array_layer_count: Some(1),
                base_mip_level,
                mip_level_count: Some(1),
                ..Default::default()
            }),
        ]
    }

    pub fn create_prefilter_cubemap_mip_views(&self) -> [wgpu::TextureView; 5] {
        [
            self.texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::Cube),
                mip_level_count: Some(1),
                base_mip_level: 0,
                ..Default::default()
            }),
            self.texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::Cube),
                mip_level_count: Some(1),
                base_mip_level: 1,
                ..Default::default()
            }),
            self.texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::Cube),
                mip_level_count: Some(1),
                base_mip_level: 2,
                ..Default::default()
            }),
            self.texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::Cube),
                mip_level_count: Some(1),
                base_mip_level: 3,
                ..Default::default()
            }),
            self.texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::Cube),
                mip_level_count: Some(1),
                base_mip_level: 4,
                ..Default::default()
            }),
        ]
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
