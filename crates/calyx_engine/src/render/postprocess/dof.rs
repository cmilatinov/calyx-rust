//! The depth of field pass: blurs the scene by distance from a focus plane.

use super::{
    CameraPlanes, NoPipeline, PassContext, PassInputs, PassIo, PostProcessPass,
    PostProcessSettings, PostProcessStage, SceneInput,
};
use crate::assets::texture::Texture;
use crate::component::ComponentDepthOfField;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::{CachedBindGroup, PipelineOptions, RenderUtils, SceneRendererError, Shader};
use egui_wgpu::wgpu;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct DofUniform {
    camera: CameraPlanes,
    focus_distance: f32,
    focus_range: f32,
    max_blur: f32,
    radius_scale: f32,
}

/// Depth of field in the `Hdr` stage. It computes each pixel's signed circle
/// of confusion from linear depth into a texture it owns, then gathers a disc
/// blur scaled by it, for both the near and the far field. It runs while the
/// settings hold a [`ComponentDepthOfField`](crate::component::ComponentDepthOfField).
pub struct DepthOfFieldPass {
    coc_shader: Ref<Shader>,
    blur_shader: Ref<Shader>,
    sampler: wgpu::Sampler,
    uniform_buffer: wgpu::Buffer,
    coc_target: Option<Texture>,
    coc_bind_group: CachedBindGroup<(wgpu::BindGroupLayout, wgpu::TextureView)>,
    blur_bind_group: CachedBindGroup<(wgpu::BindGroupLayout, wgpu::TextureView, wgpu::TextureView)>,
}

impl DepthOfFieldPass {
    /// Shader asset that computes the circle of confusion.
    pub const COC_SHADER: &'static str = "shaders/postprocess/dof_coc";
    /// Shader asset that gathers the blur.
    pub const BLUR_SHADER: &'static str = "shaders/postprocess/dof_blur";
    /// Format of the circle of confusion texture: one signed radius in
    /// pixels, filterable.
    pub const COC_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;

    /// Creates the pass, or reports why its shaders did not load.
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        let coc_shader = SceneRendererError::load::<Shader>(context, Self::COC_SHADER)?;
        let blur_shader = SceneRendererError::load::<Shader>(context, Self::BLUR_SHADER)?;
        let device = context.render_context.device();
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("dof_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dof_uniform_buffer"),
            size: size_of::<DofUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self {
            coc_shader,
            blur_shader,
            sampler,
            uniform_buffer,
            coc_target: None,
            coc_bind_group: Default::default(),
            blur_bind_group: Default::default(),
        })
    }

    /// Size of the circle of confusion texture the pass holds, if any.
    pub fn coc_target_size(&self) -> Option<(u32, u32)> {
        self.coc_target.as_ref().map(|target| {
            let size = target.descriptor.size;
            (size.width, size.height)
        })
    }

    /// The circle of confusion texture at `context`'s size, created or
    /// recreated as needed.
    fn coc_target(&mut self, context: &PassContext) -> &Texture {
        if self.coc_target_size() != Some(context.size) {
            self.release_targets();
            self.coc_target = Some(Texture::new(
                context.render_context.clone(),
                &wgpu::TextureDescriptor {
                    label: Some("dof_coc"),
                    size: wgpu::Extent3d {
                        width: context.size.0,
                        height: context.size.1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: Self::COC_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                None,
                None,
                false,
            ));
        }
        self.coc_target
            .as_ref()
            .expect("the target was just created")
    }
}

impl PostProcessPass for DepthOfFieldPass {
    fn name(&self) -> &str {
        "depth_of_field"
    }

    fn stage(&self) -> PostProcessStage {
        PostProcessStage::Hdr
    }

    /// Every visible particle blurs by its own distance, so the circle of
    /// confusion reads the depth that holds them all.
    fn inputs(&self) -> PassInputs {
        SceneInput::ParticleDepth.into()
    }

    fn enabled(&self, settings: &PostProcessSettings) -> bool {
        settings.get::<ComponentDepthOfField>().is_some()
    }

    fn prepare(&mut self, context: &PassContext, settings: &PostProcessSettings) {
        let Some(dof) = settings.get::<ComponentDepthOfField>() else {
            return;
        };
        let uniform = DofUniform {
            camera: context.camera,
            focus_distance: dof.focus_distance,
            focus_range: dof.focus_range,
            max_blur: dof.blur_radius(),
            radius_scale: dof.radius_scale(),
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
        let depth = io
            .particle_depth
            .expect("the chain gives particle depth to a pass that asks for it");
        let device = context.render_context.device();
        let coc_target = self.coc_target(context).view.clone();
        let uniform_buffer = &self.uniform_buffer;

        // Circle of confusion.
        let shader = self.coc_shader.read();
        let Some(pipeline) = shader.pipeline(
            &PipelineOptions::fullscreen_builder()
                .target(RenderUtils::color_default(Self::COC_FORMAT))
                .build(),
        ) else {
            return Err(NoPipeline::of(&shader));
        };
        let layout = shader.bind_group_layouts[0].clone();
        drop(shader);
        let bind_group =
            self.coc_bind_group
                .get_or_create((layout.clone(), depth.view.clone()), || {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("dof_coc_bind_group"),
                        layout: &layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&depth.view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: uniform_buffer.as_entire_binding(),
                            },
                        ],
                    })
                });
        let mut render_pass =
            RenderUtils::fullscreen_pass(encoder, &coc_target, "Depth Of Field CoC");
        render_pass.set_pipeline(&pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        RenderUtils::draw_fullscreen(&mut render_pass);
        drop(render_pass);

        // Gather blur.
        let shader = self.blur_shader.read();
        let Some(pipeline) = shader.pipeline(
            &PipelineOptions::fullscreen_builder()
                .target(RenderUtils::color_default(io.output.descriptor.format))
                .build(),
        ) else {
            return Err(NoPipeline::of(&shader));
        };
        let layout = shader.bind_group_layouts[0].clone();
        drop(shader);
        let sampler = &self.sampler;
        let bind_group = self.blur_bind_group.get_or_create(
            (layout.clone(), io.input.view.clone(), coc_target.clone()),
            || {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("dof_blur_bind_group"),
                    layout: &layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&io.input.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&coc_target),
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
            RenderUtils::fullscreen_pass(encoder, &io.output.view, "Depth Of Field Blur");
        render_pass.set_pipeline(&pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        RenderUtils::draw_fullscreen(&mut render_pass);
        Ok(())
    }

    fn release_targets(&mut self) {
        self.coc_target = None;
        self.coc_bind_group.clear();
        self.blur_bind_group.clear();
    }
}

#[cfg(test)]
mod tests;
