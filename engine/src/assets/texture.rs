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

/// GPU texture asset with optional egui handle and mip helpers.
#[derive(TypeUuid)]
#[uuid = "8ba4ccec-85ab-45f5-b4ee-2e803ef548a2"]
pub struct Texture {
    render_context: Arc<RenderContext>,
    /// Backing wgpu texture.
    pub texture: wgpu::Texture,
    /// Default view used for sampling.
    pub view: wgpu::TextureView,
    /// Default sampler used for sampling.
    pub sampler: wgpu::Sampler,
    /// Optional egui texture handle for UI rendering.
    pub handle: Option<egui::TextureHandle>,
    /// Stored texture descriptor used to recreate derived views.
    pub descriptor: wgpu::TextureDescriptor<'static>,
    /// Stored view descriptor used to recreate derived views.
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
        let texture_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                AssetError::LoadError
                    .with_path(path)
                    .with_type(Self::asset_name())
                    .with_source("file name is missing or not valid UTF-8")
            })?;
        let reader = ImageReader::open(path)
            .map_err(|err| AssetError::LoadError.with_path(path).with_source(err))?;
        let texture_data = Self::transform_texture(
            reader
                .decode()
                .map_err(|err| AssetError::LoadError.with_path(path).with_source(err))?,
        );
        let texture_depth = texture_data.color().bytes_per_pixel() as u32;
        let texture_format = Self::texture_format(texture_data.color());
        let texture_size = wgpu::Extent3d {
            width: texture_data.width(),
            height: texture_data.height(),
            depth_or_array_layers: 1,
        };
        let mip_level_count = Self::mip_level_count(texture_size.width, texture_size.height);
        log::info!(
            "Loaded texture {} ({}x{}, {:?}, {} mips)",
            path.display(),
            texture_size.width,
            texture_size.height,
            texture_format,
            mip_level_count
        );
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

    /// Returns true when this texture can be sampled as a single 2D texture.
    pub fn is_2d(&self) -> bool {
        self.descriptor.dimension == wgpu::TextureDimension::D2
            && self.descriptor.size.depth_or_array_layers == 1
            && self.descriptor.sample_count == 1
            && matches!(
                self.view_descriptor.dimension,
                None | Some(wgpu::TextureViewDimension::D2)
            )
    }

    fn mip_level_count(width: u32, height: u32) -> u32 {
        width.max(height).ilog2() + 1
    }

    /// Creates a texture asset from an explicit descriptor and optional view and
    /// sampler settings.
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

    /// Creates a 1x1 RGBA texture initialized from the provided bytes.
    pub fn solid_color_2d(render_context: Arc<RenderContext>, label: &str, rgba: [u8; 4]) -> Self {
        let texture = Self::new(
            render_context.clone(),
            &wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            Some(wgpu::SamplerDescriptor {
                label: Some(label),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Nearest,
                ..Default::default()
            }),
            None,
            false,
        );
        render_context.queue().write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        texture
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

    /// Creates a texture view for a single mip level.
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

        let mip_shader_ref = context
            .registries
            .assets
            .read()
            .load::<Shader>("shaders/mip_generator_2d")?;
        let device = context.render_context.device();
        let queue = context.render_context.queue();
        let mip_shader = mip_shader_ref.read();
        let pipeline = mip_shader.pipeline(
            &PipelineOptions::fullscreen_builder()
                .target(RenderUtils::color_default(self.descriptor.format))
                .build(),
        );
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("texture_mip_encoder"),
        });
        let mut src_view = self.create_mip_view(0);

        for mip_level in 1..self.descriptor.mip_level_count {
            log::trace!(
                "Generating 2D mip level {} for texture {:?}",
                mip_level,
                self.descriptor.label
            );
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
                let mut render_pass = RenderUtils::fullscreen_pass(
                    &mut encoder,
                    &dst_view,
                    "Texture 2D Mip Generation",
                );
                render_pass.set_pipeline(&pipeline);
                render_pass.set_bind_group(0, &bind_group, &[]);
                RenderUtils::draw_fullscreen(&mut render_pass);
            }
            src_view = dst_view;
        }

        queue.submit(Some(encoder.finish()));
        Ok(())
    }

    /// Creates a D2-array view suitable for cubemap compute passes.
    pub fn create_cubemap_array_view(&self, mip_level: Option<u32>) -> wgpu::TextureView {
        self.texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            base_mip_level: mip_level.unwrap_or_default(),
            mip_level_count: Some(1),
            ..Default::default()
        })
    }

    /// Creates one 2D view per cubemap face.
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

    /// Generates cubemap mip levels using `mip_shader_ref`.
    pub fn generate_cubemap_mips(
        &self,
        render_context: &RenderContext,
        encoder: &mut wgpu::CommandEncoder,
        mip_shader_ref: &Ref<Shader>,
    ) {
        let device = render_context.device();
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
            let Some(kernel) = shader.compute_pipeline("compute_main") else {
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
            compute_pass.set_pipeline(&kernel.pipeline);
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
    use crate::test_support::TestAssets;
    use egui_wgpu::wgpu;

    const TOP: [u8; 4] = [255, 0, 0, 255];
    const BOTTOM: [u8; 4] = [0, 0, 255, 255];

    /// Reads the first texel of each row of `mip` of an `Rgba8Unorm` texture.
    fn read_mip_rows(texture: &Texture, mip: u32) -> Vec<[u8; 4]> {
        let device = texture.render_context.device();
        let queue = texture.render_context.queue();
        let size = texture
            .descriptor
            .size
            .mip_level_size(mip, texture.descriptor.dimension);
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
                texture: &texture.texture,
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

        let rows = read_mip_rows(&texture, 1);
        assert_eq!(rows.first(), Some(&TOP), "mip 1's top row must stay on top");
        assert_eq!(
            rows.last(),
            Some(&BOTTOM),
            "mip 1's bottom row must stay below"
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
}
