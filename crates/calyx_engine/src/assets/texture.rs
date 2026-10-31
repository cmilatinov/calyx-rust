use egui_wgpu::wgpu;
use image::{ColorType, DynamicImage, ImageReader};
use std::path::Path;
use std::sync::Arc;

use super::LoadedAsset;
use crate::assets::error::AssetError;
use crate::assets::import::{ImportedSampler, ImportedTexture, TextureFilter, TextureWrap};
use crate::assets::Asset;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::{PipelineOptions, RenderContext, RenderUtils, Shader};
use crate::utils::TypeUuid;
use crate::{self as calyx_engine};

/// How a shader slot reads a texture's 8-bit channels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextureColorSpace {
    /// Channels are data, read as stored: normals, roughness, metallic, occlusion.
    #[default]
    Linear,
    /// Channels are sRGB-encoded colors, decoded to linear when sampled: base color and
    /// emissive images.
    Srgb,
}

/// GPU texture asset with optional egui handle and mip helpers.
#[derive(TypeUuid)]
#[uuid = "8ba4ccec-85ab-45f5-b4ee-2e803ef548a2"]
pub struct Texture {
    render_context: Arc<RenderContext>,
    /// Backing wgpu texture.
    pub texture: wgpu::Texture,
    /// Default view used for sampling, reading the stored channels as they are.
    pub view: wgpu::TextureView,
    /// Copy of an 8-bit image in the sRGB form of its format, whose mips are filtered in
    /// linear light. [`Texture::texture`] keeps the mips averaged as stored, for data slots.
    pub srgb_texture: Option<wgpu::Texture>,
    /// View of [`Texture::srgb_texture`], decoding the channels from sRGB. See
    /// [`Texture::view_as`].
    pub srgb_view: Option<wgpu::TextureView>,
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
        let image = reader
            .decode()
            .map_err(|err| AssetError::LoadError.with_path(path).with_source(err))?;
        let texture = Self::from_image(game, texture_name, image)
            .map_err(|err| err.with_path(path).with_type(Self::asset_name()))?;
        log::info!(
            "Loaded texture {} ({}x{}, {:?}, {} mips)",
            path.display(),
            texture.descriptor.size.width,
            texture.descriptor.size.height,
            texture.descriptor.format,
            texture.descriptor.mip_level_count
        );
        Ok(LoadedAsset::new(texture))
    }
}

impl Texture {
    const WORKGROUP_SIZE: f32 = 8.0;

    /// Uploads `image` as a mipmapped 2D texture. An 8-bit image can be sampled both as
    /// stored and decoded from sRGB, as each material slot chooses (see [`Texture::view_as`]).
    pub fn from_image(
        game: &ReadOnlyAssetContext,
        texture_name: &str,
        image: DynamicImage,
    ) -> Result<Self, AssetError> {
        let texture_data = Self::transform_texture(image);
        let texture_depth = texture_data.color().bytes_per_pixel() as u32;
        let texture_format = Self::texture_format(texture_data.color());
        let texture_size = wgpu::Extent3d {
            width: texture_data.width(),
            height: texture_data.height(),
            depth_or_array_layers: 1,
        };
        let mip_level_count = Self::mip_level_count(texture_size.width, texture_size.height);
        let descriptor = wgpu::TextureDescriptor {
            label: Some(texture_name),
            size: texture_size,
            mip_level_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: texture_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        };
        let mut texture = Self::new(
            game.render_context.clone(),
            &descriptor,
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
        // An 8-bit image also gets a copy in the sRGB form of its format: its mips are filtered
        // after decoding, so a color image keeps its brightness when minified, while the stored
        // chain keeps the data channels averaged as they are.
        let srgb_format = texture_format.add_srgb_suffix();
        let srgb_texture = (srgb_format != texture_format).then(|| {
            game.render_context
                .device()
                .create_texture(&wgpu::TextureDescriptor {
                    format: srgb_format,
                    ..descriptor.clone()
                })
        });
        let queue = game.render_context.queue();
        for target in std::iter::once(&texture.texture).chain(&srgb_texture) {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: target,
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
        }
        texture.srgb_view = srgb_texture
            .as_ref()
            .map(|srgb_texture| srgb_texture.create_view(&texture.view_descriptor));
        texture.srgb_texture = srgb_texture;
        texture.generate_2d_mips(game)?;
        Ok(texture)
    }

    /// Uploads an imported texture's image, sampled with its wrap modes and filters. Its mips
    /// are generated with the default linear sampler, whatever its own filters are.
    pub(crate) fn from_imported(
        game: &ReadOnlyAssetContext,
        texture_name: &str,
        imported: &ImportedTexture,
    ) -> Result<Self, AssetError> {
        let mut texture = Self::from_image(game, texture_name, (*imported.image).clone())?;
        texture.set_sampler(&Self::sampler_descriptor(texture_name, &imported.sampler));
        Ok(texture)
    }

    /// The sampler descriptor of an imported sampler. A sampler without a mip filter samples
    /// only the full-size image.
    fn sampler_descriptor<'a>(
        label: &'a str,
        sampler: &ImportedSampler,
    ) -> wgpu::SamplerDescriptor<'a> {
        let address_mode = |wrap: TextureWrap| match wrap {
            TextureWrap::Repeat => wgpu::AddressMode::Repeat,
            TextureWrap::MirroredRepeat => wgpu::AddressMode::MirrorRepeat,
            TextureWrap::ClampToEdge => wgpu::AddressMode::ClampToEdge,
        };
        let filter = |filter: TextureFilter| match filter {
            TextureFilter::Nearest => wgpu::FilterMode::Nearest,
            TextureFilter::Linear => wgpu::FilterMode::Linear,
        };
        wgpu::SamplerDescriptor {
            label: Some(label),
            address_mode_u: address_mode(sampler.wrap_u),
            address_mode_v: address_mode(sampler.wrap_v),
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: filter(sampler.mag_filter),
            min_filter: filter(sampler.min_filter),
            mipmap_filter: sampler
                .mipmap_filter
                .map_or(wgpu::FilterMode::Nearest, filter),
            lod_max_clamp: if sampler.mipmap_filter.is_some() {
                32.0
            } else {
                0.0
            },
            ..Default::default()
        }
    }

    /// Samples the texture with a sampler made from `descriptor` from now on. Mips already
    /// generated keep the filtering they were made with.
    pub fn set_sampler(&mut self, descriptor: &wgpu::SamplerDescriptor) {
        self.sampler = self.render_context.device().create_sampler(descriptor);
    }

    /// The view a slot sampling in `color_space` binds: the sRGB view for
    /// [`TextureColorSpace::Srgb`] when the texture has one, else the default view. A texture
    /// without an sRGB view, such as a solid color one, holds linear values either way.
    pub fn view_as(&self, color_space: TextureColorSpace) -> &wgpu::TextureView {
        match (color_space, &self.srgb_view) {
            (TextureColorSpace::Srgb, Some(view)) => view,
            _ => &self.view,
        }
    }

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
            srgb_texture: None,
            srgb_view: None,
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

    /// `image` in a layout [`Self::texture_format`] uploads: 32-bit float RGBA for a float
    /// image, and 8-bit RGBA for every other, such as a grayscale or 16-bit one.
    fn transform_texture(texture_data: DynamicImage) -> DynamicImage {
        match texture_data.color() {
            ColorType::Rgba8 | ColorType::Rgba32F => texture_data,
            ColorType::Rgb32F => texture_data.to_rgba32f().into(),
            _ => texture_data.to_rgba8().into(),
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
            .load::<Shader>("shaders/utility/mip_generator_2d")?;
        let mip_shader = mip_shader_ref.read();
        self.generate_2d_mip_chain(context, &mip_shader, &self.texture);
        if let Some(srgb_texture) = &self.srgb_texture {
            self.generate_2d_mip_chain(context, &mip_shader, srgb_texture);
        }
        Ok(())
    }

    /// Fills mips 1 and up of `texture`, which has this texture's size and mip count, each
    /// from the one above. Views take `texture`'s own format, so an sRGB texture is filtered
    /// after decoding and its results encoded back.
    fn generate_2d_mip_chain(
        &self,
        context: &ReadOnlyAssetContext,
        mip_shader: &Shader,
        texture: &wgpu::Texture,
    ) {
        let device = context.render_context.device();
        let queue = context.render_context.queue();
        let pipeline = mip_shader.pipeline(
            &PipelineOptions::fullscreen_builder()
                .target(RenderUtils::color_default(texture.format()))
                .build(),
        );
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("texture_mip_encoder"),
        });
        let mip_view = |mip: u32| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: mip,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        let mut src_view = mip_view(0);

        for mip_level in 1..self.descriptor.mip_level_count {
            log::trace!(
                "Generating 2D mip level {} for texture {:?} ({:?})",
                mip_level,
                self.descriptor.label,
                texture.format()
            );
            let dst_view = mip_view(mip_level);
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
        let image =
            image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(SIZE, SIZE, |x, y| {
                let value = if (x + y) % 2 == 0 { 255 } else { 0 };
                image::Rgba([value, value, value, 255])
            }));

        let texture =
            Texture::from_image(&context, "test/checker", image).expect("the image uploads");

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
}
