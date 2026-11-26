//! The bloom pass: bright HDR light bleeds into its surroundings.

use super::{
    NoPipeline, PassContext, PassIo, PostProcessPass, PostProcessSettings, PostProcessStage,
};
use crate::assets::texture::Texture;
use crate::component::ComponentBloom;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::{
    CachedBindGroup, PipelineOptions, RenderContext, RenderUtils, SceneRendererError, Shader,
};
use egui_wgpu::wgpu;
use std::sync::Arc;

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
    ) -> Result<(wgpu::RenderPipeline, wgpu::BindGroupLayout), NoPipeline> {
        let shader = shader.read();
        let pipeline = shader
            .pipeline(&PipelineOptions::fullscreen_builder().target(target).build())
            .ok_or_else(|| NoPipeline::of(&shader))?;
        Ok((pipeline, shader.bind_group_layouts[0].clone()))
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
        settings.get::<ComponentBloom>().is_some()
    }

    fn prepare(&mut self, context: &PassContext, settings: &PostProcessSettings) {
        let Some(bloom) = settings.get::<ComponentBloom>() else {
            return;
        };
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

    fn record(
        &mut self,
        context: &PassContext,
        encoder: &mut wgpu::CommandEncoder,
        io: PassIo,
    ) -> Result<(), NoPipeline> {
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
            Self::pipeline(prefilter, RenderUtils::color_default(format))?;
        let (downsample, downsample_layout) =
            Self::pipeline(downsample, RenderUtils::color_default(format))?;
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
            let (upsample, upsample_layout) = Self::pipeline(upsample, Self::additive(format))?;
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
        let (composite, layout) = Self::pipeline(composite, RenderUtils::color_default(format))?;
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
        Ok(())
    }

    fn release_targets(&mut self) {
        self.mips.clear();
        self.composite_bind_group.clear();
    }
}

#[cfg(test)]
mod tests;
