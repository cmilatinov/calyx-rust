use std::path::Path;

use egui_wgpu::wgpu;
use image::{ColorType, DynamicImage};

use crate::assets::error::AssetError;
use crate::assets::Asset;
use crate::render::{PipelineOptionsBuilder, RenderContext, RenderUtils, Shader};
use crate::utils::TypeUuid;
use crate::{self as engine};

use super::{AssetRegistry, Assets, LoadedAsset};

#[derive(TypeUuid)]
#[uuid = "8ba4ccec-85ab-45f5-b4ee-2e803ef548a2"]
pub struct Texture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub handle: Option<egui::TextureHandle>,
    pub descriptor: wgpu::TextureDescriptor<'static>,
    pub view_descriptor: wgpu::TextureViewDescriptor<'static>,
}

impl Asset for Texture {
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
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST,
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

impl Texture {
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
            descriptor: wgpu::TextureDescriptor {
                label: None,
                size: texture_desc.size,
                mip_level_count: texture_desc.mip_level_count,
                sample_count: texture_desc.sample_count,
                dimension: texture_desc.dimension,
                format: texture_desc.format,
                usage: texture_desc.usage,
                view_formats: &[],
            },
            view_descriptor: wgpu::TextureViewDescriptor {
                label: None,
                format: view_desc.format,
                dimension: view_desc.dimension,
                aspect: view_desc.aspect,
                base_mip_level: view_desc.base_mip_level,
                mip_level_count: view_desc.mip_level_count,
                base_array_layer: view_desc.base_array_layer,
                array_layer_count: view_desc.array_layer_count,
            },
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

    pub fn create_mip_view(&self, mip: u32) -> wgpu::TextureView {
        self.texture.create_view(&wgpu::TextureViewDescriptor {
            base_mip_level: mip,
            mip_level_count: Some(1),
            ..Default::default()
        })
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

    pub fn generate_mips(
        &self,
        render_state: &egui_wgpu::RenderState,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        let device = &render_state.device;
        let is_cube = if let Some(wgpu::TextureViewDimension::Cube) = self.view_descriptor.dimension
        {
            true
        } else {
            false
        };
        let shader_id = if is_cube {
            "shaders/mip_generator_cube"
        } else {
            "shaders/mip_generator_2d"
        };
        let shader_ref = AssetRegistry::get().load::<Shader>(shader_id).unwrap();
        let mut shader = shader_ref.write();
        let mesh_binding = Assets::screen_space_quad().unwrap();
        let mesh = mesh_binding.read();
        let options = PipelineOptionsBuilder::default()
            .fragment_targets(if is_cube {
                vec![
                    Some(wgpu::ColorTargetState {
                        format: self.descriptor.format,
                        blend: None,
                        write_mask: Default::default(),
                    });
                    6
                ]
            } else {
                vec![Some(wgpu::ColorTargetState {
                    format: self.descriptor.format,
                    blend: None,
                    write_mask: Default::default(),
                })]
            })
            .depth_stencil(None)
            .build();
        let main_texture_view = self.texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: if is_cube {
                Some(wgpu::TextureViewDimension::Cube)
            } else {
                None
            },
            base_mip_level: 0,
            mip_level_count: Some(1),
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &shader.bind_group_layouts[0],
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&main_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        for mip in 1..self.descriptor.mip_level_count {
            let texture_views;
            if is_cube {
                texture_views = self.create_cubemap_views(Some(mip)).into();
            } else {
                texture_views = vec![self.create_mip_view(mip)];
            }
            let attachments = texture_views
                .iter()
                .map(|v| {
                    Some(wgpu::RenderPassColorAttachment {
                        view: v,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })
                })
                .collect::<Vec<_>>();
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: attachments.as_slice(),
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            shader.build_pipeline(&options);
            if let Some(pipeline) = shader.get_pipeline(&options) {
                render_pass.set_pipeline(pipeline);
                render_pass.set_bind_group(0, &bind_group, &[]);
                RenderUtils::bind_mesh_buffers(&mut render_pass, &mesh);
                RenderUtils::draw_mesh_instanced(&mut render_pass, &mesh, 0..1);
            }
        }
    }
}

impl Drop for Texture {
    fn drop(&mut self) {
        if let Some(handle) = &self.handle {
            let renderer = RenderContext::renderer().unwrap();
            renderer.write().free_texture(&handle.id());
        }
    }
}
