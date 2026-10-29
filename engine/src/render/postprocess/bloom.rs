//! The bloom pass: bright HDR light bleeds into its surroundings.

use super::{PassContext, PassIo, PostProcessPass, PostProcessSettings, PostProcessStage};
use crate as engine;
use crate::assets::texture::Texture;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::reflect::{Reflect, ReflectDefault};
use crate::render::{
    CachedBindGroup, PipelineOptions, RenderContext, RenderUtils, SceneRendererError, Shader,
};
use crate::utils::TypeUuid;
use egui_wgpu::wgpu;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Bloom: light above a threshold is blurred and added back onto the scene.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "09244af2-9225-4a2d-bb1b-8ba6ee47e297"]
#[reflect(Default)]
#[serde(default)]
pub struct BloomSettings {
    /// Whether bloom runs. Off by default, so scenes saved before bloom
    /// existed look the same.
    pub enabled: bool,
    /// Linear HDR brightness, of a pixel's brightest channel, above which
    /// light blooms.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub threshold: f32,
    /// Softness of the threshold, as a fraction of it: light starts to bloom
    /// at `threshold * (1 - knee)` and blooms fully from
    /// `threshold * (1 + knee)`. Zero is a hard cut.
    #[reflect_attr(min = 0.0, max = 1.0, speed = 0.01)]
    pub knee: f32,
    /// How much of the blurred light is added back onto the scene.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub intensity: f32,
    /// How many half-size mips the light is blurred through. Each one doubles
    /// how far the glow spreads. Clamped to `1..=8`.
    pub radius: u32,
}

impl Default for BloomSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold: 1.0,
            knee: 0.5,
            intensity: 0.5,
            radius: 6,
        }
    }
}

impl BloomSettings {
    /// Most mips `radius` selects.
    pub const MAX_RADIUS: u32 = 8;

    /// How many mips bloom blurs through at `size`: `radius`, clamped
    /// to `1..=MAX_RADIUS` and to the halvings `size` allows before a side
    /// reaches one pixel.
    pub fn mip_count(&self, size: (u32, u32)) -> usize {
        let halvings = size.0.min(size.1).max(2).ilog2();
        self.radius.clamp(1, Self::MAX_RADIUS).min(halvings) as usize
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct BloomUniform {
    threshold: f32,
    knee: f32,
    intensity: f32,
    /// One over the mip count. The prefilter scales the light by it, so the
    /// sum of every mip that the upsample builds stays within half-float
    /// range.
    mip_scale: f32,
}

type ViewKey = (wgpu::BindGroupLayout, wgpu::TextureView);

/// One texture of the mip chain, with the bind groups that read it.
struct Mip {
    texture: Texture,
    /// Reads the previous mip, or the input for the first, to downsample.
    downsample: CachedBindGroup<ViewKey>,
    /// Reads the next mip to upsample it into this one.
    upsample: CachedBindGroup<ViewKey>,
}

/// Blurs light above a soft-knee threshold through a chain of half-size mips
/// and adds it back onto the scene. Runs in the `Hdr` stage.
///
/// The prefilter downsamples the input into the first mip and keeps only
/// light above the threshold. Each later mip downsamples the one before it
/// with a 13-tap filter. Going back up, each mip is tent-filtered and added
/// onto the next larger one, so the first mip ends up holding every mip's
/// blur. The prefilter already divides the light by the mip count, so that
/// sum stays within half-float range. The composite adds it onto the input,
/// scaled by the intensity.
///
/// The pass owns its mip chain, separate from the chain's pool targets. It
/// creates it on the first frame bloom runs, sized from the chain, and frees
/// it on resize and on any frame bloom does not run.
pub struct BloomPass {
    prefilter: Ref<Shader>,
    downsample: Ref<Shader>,
    upsample: Ref<Shader>,
    composite: Ref<Shader>,
    sampler: wgpu::Sampler,
    uniform_buffer: wgpu::Buffer,
    mip_count: usize,
    mips: Vec<Mip>,
    composite_bind_group:
        CachedBindGroup<(wgpu::BindGroupLayout, wgpu::TextureView, wgpu::TextureView)>,
}

impl BloomPass {
    /// Shader of the threshold and first downsample.
    pub const PREFILTER_SHADER: &'static str = "shaders/postprocess/bloom_prefilter";
    /// Shader of each later downsample.
    pub const DOWNSAMPLE_SHADER: &'static str = "shaders/postprocess/bloom_downsample";
    /// Shader of each upsample.
    pub const UPSAMPLE_SHADER: &'static str = "shaders/postprocess/bloom_upsample";
    /// Shader that adds the blur onto the scene.
    pub const COMPOSITE_SHADER: &'static str = "shaders/postprocess/bloom_composite";

    /// Creates the pass, or reports why a shader did not load. It allocates
    /// no texture until bloom first runs.
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        let device = context.render_context.device();
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("bloom_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bloom_uniform_buffer"),
            size: size_of::<BloomUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self {
            prefilter: SceneRendererError::load::<Shader>(context, Self::PREFILTER_SHADER)?,
            downsample: SceneRendererError::load::<Shader>(context, Self::DOWNSAMPLE_SHADER)?,
            upsample: SceneRendererError::load::<Shader>(context, Self::UPSAMPLE_SHADER)?,
            composite: SceneRendererError::load::<Shader>(context, Self::COMPOSITE_SHADER)?,
            sampler,
            uniform_buffer,
            mip_count: 0,
            mips: Vec::new(),
            composite_bind_group: Default::default(),
        })
    }

    /// The sizes of the mip textures the pass holds, largest first.
    pub fn mip_sizes(&self) -> Vec<(u32, u32)> {
        self.mips
            .iter()
            .map(|mip| {
                let size = mip.texture.descriptor.size;
                (size.width, size.height)
            })
            .collect()
    }

    /// Makes the mip chain match `size`, `format` and the prepared mip count,
    /// keeping the mips that already do.
    fn fit_mips(
        &mut self,
        render_context: &Arc<RenderContext>,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) {
        self.mips.truncate(self.mip_count);
        for level in 0..self.mip_count {
            let mip_size = (
                (size.0 >> (level + 1)).max(1),
                (size.1 >> (level + 1)).max(1),
            );
            let matches = self.mips.get(level).is_some_and(|mip| {
                let descriptor = &mip.texture.descriptor;
                (descriptor.size.width, descriptor.size.height) == mip_size
                    && descriptor.format == format
            });
            if matches {
                continue;
            }
            let mip = Mip {
                texture: Self::create_mip(render_context.clone(), mip_size, format),
                downsample: Default::default(),
                upsample: Default::default(),
            };
            if level < self.mips.len() {
                self.mips[level] = mip;
            } else {
                self.mips.push(mip);
            }
        }
    }

    fn create_mip(
        render_context: Arc<RenderContext>,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> Texture {
        Texture::new(
            render_context,
            &wgpu::TextureDescriptor {
                label: Some("bloom_mip"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            None,
            None,
            false,
        )
    }

    /// A bind group of `texture`, the sampler and, when the shader has a third
    /// binding, the uniform.
    fn sampling_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        texture: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        uniform: Option<&wgpu::Buffer>,
    ) -> wgpu::BindGroup {
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(texture),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ];
        if let Some(uniform) = uniform {
            entries.push(wgpu::BindGroupEntry {
                binding: 2,
                resource: uniform.as_entire_binding(),
            });
        }
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bloom_bind_group"),
            layout,
            entries: &entries,
        })
    }

    /// The pipeline of `shader` drawing into `format`, and its bind group
    /// layout.
    fn pipeline(
        shader: &Ref<Shader>,
        target: wgpu::ColorTargetState,
    ) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout) {
        let shader = shader.read();
        let pipeline =
            shader.pipeline(&PipelineOptions::fullscreen_builder().target(target).build());
        (pipeline, shader.bind_group_layouts[0].clone())
    }

    /// Adds the source color onto what the target holds.
    fn additive(format: wgpu::TextureFormat) -> wgpu::ColorTargetState {
        wgpu::ColorTargetState {
            format,
            blend: Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::REPLACE,
            }),
            write_mask: wgpu::ColorWrites::COLOR,
        }
    }
}

impl PostProcessPass for BloomPass {
    fn name(&self) -> &str {
        "bloom"
    }

    fn stage(&self) -> PostProcessStage {
        PostProcessStage::Hdr
    }

    fn enabled(&self, settings: &PostProcessSettings) -> bool {
        settings.bloom.enabled
    }

    fn prepare(&mut self, context: &PassContext, settings: &PostProcessSettings) {
        let bloom = &settings.bloom;
        self.mip_count = bloom.mip_count(context.size);
        let threshold = bloom.threshold.max(0.0);
        let uniform = BloomUniform {
            threshold,
            knee: threshold * bloom.knee.clamp(0.0, 1.0),
            intensity: bloom.intensity.max(0.0),
            mip_scale: 1.0 / self.mip_count as f32,
        };
        context.render_context.queue().write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::bytes_of(&uniform),
        );
    }

    fn record(&mut self, context: &PassContext, encoder: &mut wgpu::CommandEncoder, io: PassIo) {
        let format = io.output.descriptor.format;
        self.fit_mips(context.render_context, context.size, format);
        let device = context.render_context.device();
        let Self {
            prefilter,
            downsample,
            upsample,
            composite,
            sampler,
            uniform_buffer,
            mips,
            composite_bind_group,
            ..
        } = self;

        // Threshold and downsample the input into the first mip, then each
        // mip into the next.
        let (prefilter, prefilter_layout) =
            Self::pipeline(prefilter, RenderUtils::color_default(format));
        let (downsample, downsample_layout) =
            Self::pipeline(downsample, RenderUtils::color_default(format));
        for level in 0..mips.len() {
            let (pipeline, layout, source, uniform) = if level == 0 {
                (
                    &prefilter,
                    &prefilter_layout,
                    &io.input.view,
                    Some(&*uniform_buffer),
                )
            } else {
                (
                    &downsample,
                    &downsample_layout,
                    &mips[level - 1].texture.view,
                    None,
                )
            };
            let (layout, source) = (layout.clone(), source.clone());
            let bind_group = mips[level]
                .downsample
                .get_or_create((layout.clone(), source.clone()), || {
                    Self::sampling_bind_group(device, &layout, &source, sampler, uniform)
                })
                .clone();
            let mut render_pass = RenderUtils::fullscreen_pass(
                encoder,
                &mips[level].texture.view,
                "Bloom Downsample",
            );
            render_pass.set_pipeline(pipeline);
            render_pass.set_bind_group(0, &bind_group, &[]);
            RenderUtils::draw_fullscreen(&mut render_pass);
        }

        // Upsample each mip and add it onto the next larger one. A single mip
        // has nothing to upsample, so it needs no blending pipeline.
        if mips.len() > 1 {
            let (upsample, upsample_layout) = Self::pipeline(upsample, Self::additive(format));
            for level in (1..mips.len()).rev() {
                let source = mips[level].texture.view.clone();
                let layout = upsample_layout.clone();
                let bind_group = mips[level - 1]
                    .upsample
                    .get_or_create((layout.clone(), source.clone()), || {
                        Self::sampling_bind_group(device, &layout, &source, sampler, None)
                    })
                    .clone();
                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Bloom Upsample"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &mips[level - 1].texture.view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                render_pass.set_pipeline(&upsample);
                render_pass.set_bind_group(0, &bind_group, &[]);
                RenderUtils::draw_fullscreen(&mut render_pass);
            }
        }

        // Add the first mip, holding every mip's blur, onto the input.
        let (composite, layout) = Self::pipeline(composite, RenderUtils::color_default(format));
        let bloom = &mips[0].texture.view;
        let bind_group = composite_bind_group.get_or_create(
            (layout.clone(), io.input.view.clone(), bloom.clone()),
            || {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("bloom_composite_bind_group"),
                    layout: &layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&io.input.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(bloom),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: uniform_buffer.as_entire_binding(),
                        },
                    ],
                })
            },
        );
        let mut render_pass =
            RenderUtils::fullscreen_pass(encoder, &io.output.view, "Bloom Composite");
        render_pass.set_pipeline(&composite);
        render_pass.set_bind_group(0, bind_group, &[]);
        RenderUtils::draw_fullscreen(&mut render_pass);
    }

    fn release_targets(&mut self) {
        self.mips.clear();
        self.composite_bind_group.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::ComponentGlobalPostProcessVolume;
    use crate::render::postprocess::{
        ChainPlan, ChainTargets, PassTarget, PlannedPass, PostProcessChain,
    };
    use crate::render::scene_renderer::half_to_f32;
    use crate::render::{Camera, SceneRenderer, SceneRendererOptions};
    use crate::test_support::{SceneBuilder, TestAssets};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const SIZE: (u32, u32) = (16, 16);
    const BYTES_PER_ROW: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    /// The largest finite half float.
    const HALF_MAX: f32 = 65_504.0;

    /// `value` as a half float. Test values are zero or normal halves.
    fn f32_to_half(value: f32) -> u16 {
        if value == 0.0 {
            return 0;
        }
        let bits = value.to_bits();
        let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
        assert!((1..31).contains(&exponent), "{value} is not a normal half");
        (((bits >> 16) & 0x8000) | ((exponent as u32) << 10) | ((bits & 0x7f_ffff) >> 13)) as u16
    }

    fn color_target(render_context: &Arc<RenderContext>, size: (u32, u32)) -> Texture {
        format_target(render_context, size, wgpu::TextureFormat::Rgba16Float)
    }

    fn format_target(
        render_context: &Arc<RenderContext>,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> Texture {
        Texture::new(
            render_context.clone(),
            &wgpu::TextureDescriptor {
                label: Some("bloom_test_target"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            None,
            None,
            false,
        )
    }

    fn depth_target(render_context: &Arc<RenderContext>, size: (u32, u32)) -> Texture {
        Texture::new(
            render_context.clone(),
            &wgpu::TextureDescriptor {
                label: Some("bloom_test_depth"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            None,
            None,
            false,
        )
    }

    /// An `Rgba16Float` texture of `size` holding `pixel(x, y)`.
    fn upload(
        render_context: &Arc<RenderContext>,
        size: (u32, u32),
        pixel: impl Fn(u32, u32) -> [f32; 4],
    ) -> Texture {
        assert!(size.0 * 8 <= BYTES_PER_ROW);
        let texture = color_target(render_context, size);
        let mut data = vec![0u8; (BYTES_PER_ROW * size.1) as usize];
        for y in 0..size.1 {
            for x in 0..size.0 {
                let start = (y * BYTES_PER_ROW + x * 8) as usize;
                for (channel, value) in pixel(x, y).into_iter().enumerate() {
                    data[start + channel * 2..start + channel * 2 + 2]
                        .copy_from_slice(&f32_to_half(value).to_le_bytes());
                }
            }
        }
        render_context.queue().write_texture(
            texture.texture.as_image_copy(),
            &data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(BYTES_PER_ROW),
                rows_per_image: Some(size.1),
            },
            texture.descriptor.size,
        );
        texture
    }

    /// An `Rgba32Float` texture of `size` holding `pixel` everywhere.
    fn upload_f32(
        render_context: &Arc<RenderContext>,
        size: (u32, u32),
        pixel: [f32; 4],
    ) -> Texture {
        let texture = format_target(render_context, size, wgpu::TextureFormat::Rgba32Float);
        let data: Vec<u8> = (0..size.0 * size.1)
            .flat_map(|_| bytemuck::cast::<[f32; 4], [u8; 16]>(pixel))
            .collect();
        render_context.queue().write_texture(
            texture.texture.as_image_copy(),
            &data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.0 * 16),
                rows_per_image: Some(size.1),
            },
            texture.descriptor.size,
        );
        texture
    }

    /// Every texel of an `Rgba16Float` or `Rgba32Float` texture, row by row.
    fn read_all(render_context: &RenderContext, texture: &Texture) -> Vec<[f32; 4]> {
        let size = texture.descriptor.size;
        let wide = texture.descriptor.format == wgpu::TextureFormat::Rgba32Float;
        let texel_bytes = if wide { 16 } else { 8 };
        assert!(size.width * texel_bytes <= BYTES_PER_ROW);
        let device = render_context.device();
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bloom_test_readback"),
            size: (BYTES_PER_ROW * size.height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(BYTES_PER_ROW),
                    rows_per_image: Some(size.height),
                },
            },
            size,
        );
        render_context.queue().submit(Some(encoder.finish()));
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |result| result.expect("map failed"));
        let _ = device.poll(wgpu::Maintain::Wait);
        let bytes = slice.get_mapped_range();
        let mut texels = Vec::new();
        for y in 0..size.height {
            for x in 0..size.width {
                let start = (y * BYTES_PER_ROW + x * texel_bytes) as usize;
                texels.push(std::array::from_fn(|channel| {
                    if wide {
                        let at = start + channel * 4;
                        f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
                    } else {
                        let at = start + channel * 2;
                        half_to_f32(u16::from_le_bytes([bytes[at], bytes[at + 1]]))
                    }
                }));
            }
        }
        drop(bytes);
        buffer.unmap();
        texels
    }

    fn bloom(intensity: f32, radius: u32) -> PostProcessSettings {
        PostProcessSettings {
            bloom: BloomSettings {
                enabled: true,
                intensity,
                radius,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Runs a chain of bloom alone over `source` and returns its output.
    fn run_bloom(
        context: &ReadOnlyAssetContext,
        settings: &PostProcessSettings,
        source: &Texture,
    ) -> Vec<[f32; 4]> {
        let render_context = context.render_context.clone();
        let mut chain = PostProcessChain::new(context).expect("chain shaders load");
        chain.push(Box::new(BloomPass::new(context).expect("bloom loads")));
        let size = source.descriptor.size;
        let size = (size.width, size.height);
        let output = format_target(&render_context, size, source.descriptor.format);
        let depth = depth_target(&render_context, size);
        let mut encoder = render_context
            .device()
            .create_command_encoder(&Default::default());
        let plan = chain.run(
            &render_context,
            &mut encoder,
            settings,
            ChainTargets {
                source,
                output: &output,
                depth: &depth,
            },
        );
        assert_eq!(plan.passes.len(), 1, "bloom runs");
        render_context.queue().submit(Some(encoder.finish()));
        read_all(&render_context, &output)
    }

    #[test]
    fn a_pixel_above_the_threshold_spreads_energy_into_its_neighbours() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let source = upload(&context.render_context, SIZE, |x, y| {
            let value = if (x, y) == (8, 8) { 20.0 } else { 0.0 };
            [value, value, value, 1.0]
        });

        let output = run_bloom(&context, &bloom(1.0, 3), &source);

        let at = |x: u32, y: u32| output[(y * SIZE.0 + x) as usize];
        assert!(at(8, 8)[0] >= 20.0, "the bright pixel keeps its own light");
        for (x, y) in [(7, 8), (9, 8), (8, 7), (8, 9), (6, 6), (8, 11)] {
            assert!(
                at(x, y)[0] > 0.01,
                "light spreads to ({x}, {y}): got {:?}",
                at(x, y)
            );
            assert_eq!(at(x, y)[0], at(x, y)[2], "a white light blooms white");
        }
        assert!(
            at(7, 8)[0] > at(5, 8)[0] && at(5, 8)[0] > at(1, 8)[0],
            "the glow fades with distance"
        );
        for texel in &output {
            assert_eq!(texel[3], 1.0, "bloom keeps alpha");
        }
    }

    #[test]
    fn a_bright_image_blooms_without_overflowing_the_half_float_mips() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        // Each of the four mips holds about 20,000; summed unnormalized they
        // would pass the half-float maximum of 65,504.
        let source = upload(&context.render_context, SIZE, |_, _| {
            [20_000.0, 20_000.0, 20_000.0, 1.0]
        });

        let output = run_bloom(&context, &bloom(0.5, 4), &source);

        // A flat image blurs to itself: input + (input - threshold) * intensity.
        let expected = 20_000.0 + (20_000.0 - 1.0) * 0.5;
        for texel in &output {
            for &channel in &texel[..3] {
                assert!(
                    (channel - expected).abs() <= expected * 0.01,
                    "expected about {expected}, got {texel:?}"
                );
            }
            assert_eq!(texel[3], 1.0, "bloom keeps alpha");
        }
    }

    #[test]
    fn a_composite_brighter_than_half_float_saturates_instead_of_overflowing() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        // The scene's HDR targets are `Rgba16Float`, but how a GPU stores a
        // value past their range is up to it: some saturate, some write
        // infinity. An `Rgba32Float` chain shows what the composite writes.
        if !context
            .render_context
            .device()
            .features()
            .contains(wgpu::Features::FLOAT32_FILTERABLE)
        {
            eprintln!("skipping: the test adapter cannot filter Rgba32Float targets");
            return;
        }
        // 50,000 + 49,999 * 0.5 is about 75,000, past the half-float maximum.
        // One mip, since `Rgba32Float` cannot blend the upsample.
        let source = upload_f32(
            &context.render_context,
            SIZE,
            [50_000.0, 50_000.0, 50_000.0, 1.0],
        );

        let output = run_bloom(&context, &bloom(0.5, 1), &source);

        for texel in &output {
            for &channel in &texel[..3] {
                assert_eq!(
                    channel, HALF_MAX,
                    "the composite saturates at the half-float maximum, got {texel:?}"
                );
            }
            assert_eq!(texel[3], 1.0, "bloom keeps alpha");
        }
    }

    #[test]
    fn an_image_below_the_threshold_passes_through_unchanged() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        // Brightest channel 30 / 64, below where the default soft knee starts
        // at threshold * (1 - knee) = 0.5.
        let pixel = |x: u32, y: u32| {
            [
                (x + y) as f32 / 64.0,
                x as f32 / 64.0,
                y as f32 / 64.0,
                (x % 4) as f32 / 4.0,
            ]
        };
        let source = upload(&context.render_context, SIZE, pixel);

        let output = run_bloom(&context, &bloom(1.0, 4), &source);

        for y in 0..SIZE.1 {
            for x in 0..SIZE.0 {
                assert_eq!(
                    output[(y * SIZE.0 + x) as usize],
                    pixel(x, y),
                    "({x}, {y}) is unchanged"
                );
            }
        }
    }

    #[test]
    fn with_bloom_disabled_the_plan_is_the_tone_map_only_plan() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let mut builder = SceneBuilder::new(context.registries.clone());
        let volume = builder.spawn("volume");
        builder.add_component(volume, ComponentGlobalPostProcessVolume::default());
        let scene = builder.finish();
        let mut renderer = SceneRenderer::new(
            &context,
            SceneRendererOptions {
                samples: 1,
                ..Default::default()
            },
            SIZE,
        )
        .expect("scene renderer should load its assets");
        let camera = Camera::new(1.0, 60f32.to_radians(), 0.1, 100.0);
        renderer.render_scene_base(&camera, &Default::default(), &scene, None, None);
        renderer.finalize_scene();

        let names: Vec<_> = renderer
            .post_process()
            .passes()
            .iter()
            .map(|pass| pass.name())
            .collect();
        assert_eq!(
            names,
            [
                "depth_of_field",
                "bloom",
                "tonemap",
                "color_grading",
                "fxaa"
            ],
            "the renderer runs bloom after depth of field"
        );
        assert_eq!(
            *renderer.last_post_process_plan(),
            ChainPlan {
                passes: vec![PlannedPass {
                    pass: 2,
                    input: PassTarget::Source,
                    output: PassTarget::Output,
                }],
                skipped: vec![],
                intermediates: 0,
                depth: false,
            }
        );
        assert_eq!(renderer.post_process().pool().allocated(), 0);
    }

    #[test]
    fn the_mip_chain_follows_the_chain_size_and_bloom_settings() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let render_context = context.render_context.clone();
        let mut pass = BloomPass::new(&context).expect("bloom loads");
        assert!(pass.mip_sizes().is_empty(), "nothing before bloom runs");

        let record = |pass: &mut BloomPass, size: (u32, u32), radius: u32| {
            let input = color_target(&render_context, size);
            let output = color_target(&render_context, size);
            let pass_context = PassContext {
                render_context: &render_context,
                size,
                camera: Default::default(),
            };
            pass.prepare(&pass_context, &bloom(1.0, radius));
            let mut encoder = render_context
                .device()
                .create_command_encoder(&Default::default());
            pass.record(
                &pass_context,
                &mut encoder,
                PassIo {
                    input: &input,
                    output: &output,
                    depth: None,
                },
            );
            render_context.queue().submit(Some(encoder.finish()));
            pass.mip_sizes()
        };

        assert_eq!(record(&mut pass, (16, 16), 3), [(8, 8), (4, 4), (2, 2)]);
        assert_eq!(
            record(&mut pass, (32, 12), 3),
            [(16, 6), (8, 3), (4, 1)],
            "a new chain size resizes every mip"
        );
        assert_eq!(
            record(&mut pass, (32, 12), 8),
            [(16, 6), (8, 3), (4, 1)],
            "the radius stops where a side would drop below one pixel"
        );
        assert_eq!(record(&mut pass, (32, 12), 1), [(16, 6)]);
        pass.release_targets();
        assert!(pass.mip_sizes().is_empty(), "releasing frees every mip");
    }

    #[test]
    fn settings_saved_before_bloom_load_with_bloom_off() {
        let saved = r#"{ "tonemapping": { "exposure_ev": 1.5, "preset": "Aces" } }"#;
        let settings: PostProcessSettings = serde_json::from_str(saved).expect("settings load");
        assert_eq!(settings.tonemapping.exposure_ev, 1.5);
        assert_eq!(settings.bloom, BloomSettings::default());
        assert!(!settings.bloom.enabled, "bloom is off by default");
    }

    /// Counts how often the chain asks it to release its targets.
    struct ReleaseCounter {
        releases: Arc<AtomicUsize>,
    }

    impl PostProcessPass for ReleaseCounter {
        fn name(&self) -> &str {
            "release_counter"
        }
        fn stage(&self) -> PostProcessStage {
            PostProcessStage::Hdr
        }
        fn enabled(&self, settings: &PostProcessSettings) -> bool {
            settings.bloom.enabled
        }
        fn prepare(&mut self, _: &PassContext, _: &PostProcessSettings) {}
        fn record(&mut self, _: &PassContext, _: &mut wgpu::CommandEncoder, _: PassIo) {}
        fn release_targets(&mut self) {
            self.releases.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn the_chain_releases_pass_targets_on_resize_and_while_a_pass_is_disabled() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let render_context = context.render_context.clone();
        let releases = Arc::new(AtomicUsize::new(0));
        let mut chain = PostProcessChain::new(&context).expect("chain shaders load");
        chain.push(Box::new(ReleaseCounter {
            releases: releases.clone(),
        }));
        let source = color_target(&render_context, SIZE);
        let output = color_target(&render_context, SIZE);
        let depth = depth_target(&render_context, SIZE);
        let run = |chain: &mut PostProcessChain, settings: &PostProcessSettings| {
            let mut encoder = render_context
                .device()
                .create_command_encoder(&Default::default());
            chain.run(
                &render_context,
                &mut encoder,
                settings,
                ChainTargets {
                    source: &source,
                    output: &output,
                    depth: &depth,
                },
            );
        };

        run(&mut chain, &bloom(1.0, 3));
        assert_eq!(
            releases.load(Ordering::SeqCst),
            0,
            "a running pass keeps them"
        );
        run(&mut chain, &PostProcessSettings::default());
        assert_eq!(
            releases.load(Ordering::SeqCst),
            1,
            "a disabled pass frees them"
        );
        chain.clear_targets();
        assert_eq!(releases.load(Ordering::SeqCst), 2, "a resize frees them");
    }
}
