use egui_wgpu::wgpu;
use image::{ColorType, DynamicImage, ImageReader};
use std::path::Path;
use std::sync::Arc;

use super::LoadedAsset;
use crate::assets::error::AssetError;
use crate::assets::Asset;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::{PipelineOptions, RenderContext, RenderUtils, Shader};
use crate::utils::TypeUuid;
use crate::{self as engine};

#[derive(TypeUuid)]
#[uuid = "8ba4ccec-85ab-45f5-b4ee-2e803ef548a2"]
pub struct Texture {
    render_context: Arc<RenderContext>,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub handle: Option<egui::TextureHandle>,
    pub descriptor: wgpu::TextureDescriptor<'static>,
    pub view_descriptor: wgpu::TextureViewDescriptor<'static>,
}

impl Asset for Texture {
    fn asset_name() -> &'static str
    where
        Self: Sized,
    {
        "Texture"
    }

    fn file_extensions() -> &'static [&'static str] {
        &["png", "jpg", "jpeg", "webp"]
    }

    fn from_file(
        game: &ReadOnlyAssetContext,
        path: &Path,
    ) -> Result<LoadedAsset<Self>, AssetError> {
        let reader = ImageReader::open(path)
            .map_err(|err| AssetError::LoadError.with_path(path).with_source(err))?;
        let texture_data = Self::transform_texture(
            reader
                .decode()
                .map_err(|err| AssetError::LoadError.with_path(path).with_source(err))?,
        );
        let texture_depth = texture_data.color().bytes_per_pixel() as u32;
        let texture_format = Self::texture_format(texture_data.color());
        let texture_name = path.file_name().unwrap().to_str().unwrap();
        let texture_size = wgpu::Extent3d {
            width: texture_data.width(),
            height: texture_data.height(),
            depth_or_array_layers: 1,
        };
        let mip_level_count = Self::mip_level_count(texture_size.width, texture_size.height);
        let texture = Self::new(
            game.render_context.clone(),
            &wgpu::TextureDescriptor {
                label: Some(texture_name),
                size: texture_size,
                mip_level_count,
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
        let queue = game.render_context.queue();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            texture_data.as_bytes(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(texture_depth * texture_data.width()),
                rows_per_image: Some(texture_data.height()),
            },
            texture_size,
        );
        texture.generate_2d_mips(game)?;
        Ok(LoadedAsset::new(texture))
    }
}

impl Texture {
    const WORKGROUP_SIZE: f32 = 8.0;

    fn mip_level_count(width: u32, height: u32) -> u32 {
        width.max(height).ilog2() + 1
    }

    pub fn new(
        render_context: Arc<RenderContext>,
        texture_desc: &wgpu::TextureDescriptor,
        sampler_desc: Option<wgpu::SamplerDescriptor>,
        view_desc: Option<wgpu::TextureViewDescriptor>,
        create_handle: bool,
    ) -> Self {
        let device = render_context.device();
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
        let view_desc = view_desc.unwrap_or_default();
        let texture = device.create_texture(texture_desc);
        let sampler = device.create_sampler(&sampler_desc);
        let view = texture.create_view(&view_desc);
        let handle = if create_handle && !render_context.is_headless() {
            let renderer = render_context.renderer();
            let tex_mgr = render_context.texture_manager();
            let texture_id =
                renderer
                    .write()
                    .register_native_texture(device, &view, wgpu::FilterMode::Linear);
            Some(egui::TextureHandle::new(tex_mgr, texture_id))
        } else {
            None
        };
        Self {
            render_context,
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
                usage: None,
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

    fn generate_2d_mips(&self, context: &ReadOnlyAssetContext) -> Result<(), AssetError> {
        if self.descriptor.dimension != wgpu::TextureDimension::D2
            || self.descriptor.size.depth_or_array_layers != 1
            || self.descriptor.mip_level_count <= 1
        {
            return Ok(());
        }

        let (mip_shader_ref, screen_space_quad_ref) = {
            let asset_registry = context.registries.assets.read();
            (
                asset_registry.load::<Shader>("shaders/mip_generator_2d")?,
                asset_registry
                    .screen_space_quad()
                    .ok_or_else(|| AssetError::NotFound.with_source("screen_space_quad"))?,
            )
        };
        let device = context.render_context.device();
        let queue = context.render_context.queue();
        let mut mip_shader = mip_shader_ref.write();
        let mut screen_space_quad = screen_space_quad_ref.write();
        let options = PipelineOptions::builder()
            .fragment_targets(vec![Some(wgpu::ColorTargetState {
                format: self.descriptor.format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })])
            .depth_stencil(None)
            .build();
        mip_shader.build_pipeline(&options);
        let Some(pipeline) = mip_shader.get_pipeline(&options) else {
            return Ok(());
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("texture_mip_encoder"),
        });
        let mut src_view = self.create_mip_view(0);

        for mip_level in 1..self.descriptor.mip_level_count {
            let dst_view = self.create_mip_view(mip_level);
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("texture_mip_bind_group"),
                layout: &mip_shader.bind_group_layouts[0],
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&src_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            {
                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Texture 2D Mip Generation"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &dst_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                render_pass.set_pipeline(pipeline);
                render_pass.set_bind_group(0, &bind_group, &[]);
                RenderUtils::render_mesh(device, queue, &mut render_pass, &mut screen_space_quad);
            }
            src_view = dst_view;
        }

        queue.submit(Some(encoder.finish()));
        Ok(())
    }

    pub fn create_cubemap_array_view(&self, mip_level: Option<u32>) -> wgpu::TextureView {
        self.texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            base_mip_level: mip_level.unwrap_or_default(),
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

    pub fn generate_cubemap_mips(
        &self,
        render_state: &egui_wgpu::RenderState,
        encoder: &mut wgpu::CommandEncoder,
        mip_shader_ref: &Ref<Shader>,
    ) {
        let device = &render_state.device;
        let Some(wgpu::TextureViewDimension::Cube) = self.view_descriptor.dimension else {
            return;
        };
        let shader = mip_shader_ref.write();
        let mut src_texture_view = self.texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            base_mip_level: 0,
            mip_level_count: Some(1),
            ..Default::default()
        });
        for mip_level in 1..self.descriptor.mip_level_count {
            let Some(pipeline) = shader.get_compute_pipeline() else {
                continue;
            };
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            let dst_texture_view = self.texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                base_mip_level: mip_level,
                mip_level_count: Some(1),
                ..Default::default()
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &shader.bind_group_layouts[0],
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&src_texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&dst_texture_view),
                    },
                ],
            });
            let workgroup_count_x = ((self.descriptor.size.width >> mip_level) as f32
                / Self::WORKGROUP_SIZE)
                .ceil() as u32;
            let workgroup_count_y = ((self.descriptor.size.height >> mip_level) as f32
                / Self::WORKGROUP_SIZE)
                .ceil() as u32;
            compute_pass.set_pipeline(pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);
            compute_pass.dispatch_workgroups(workgroup_count_x, workgroup_count_y, 6);
            src_texture_view = dst_texture_view;
        }
    }
}

impl Drop for Texture {
    fn drop(&mut self) {
        if let Some(handle) = &self.handle {
            let renderer = self.render_context.renderer();
            renderer.write().free_texture(&handle.id());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Texture;

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
}
