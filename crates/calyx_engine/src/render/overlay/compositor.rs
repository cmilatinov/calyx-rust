//! The overlay target and its composite over the post-processed scene.

use super::{OverlayContext, OverlayPass};
use crate::assets::texture::Texture;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::{
    CachedBindGroup, PipelineOptions, RenderContext, RenderUtils, SceneRendererError, Shader,
};
use egui_wgpu::wgpu;
use std::sync::Arc;

/// The overlay target: multisampled at the scene's sample count, with the
/// single-sample texture it resolves into. At one sample the overlays draw
/// into the single-sample texture directly.
struct OverlayTargets {
    multisampled: Option<Texture>,
    resolved: Texture,
}

impl OverlayTargets {
    fn new(
        render_context: &Arc<RenderContext>,
        size: wgpu::Extent3d,
        format: wgpu::TextureFormat,
        samples: u32,
    ) -> Self {
        let create = |label, sample_count, usage| {
            Texture::new(
                render_context.clone(),
                &wgpu::TextureDescriptor {
                    label: Some(label),
                    size,
                    mip_level_count: 1,
                    sample_count,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                },
                None,
                None,
                false,
            )
        };
        Self {
            multisampled: (samples > 1).then(|| {
                create(
                    "overlay_target_msaa",
                    samples,
                    wgpu::TextureUsages::RENDER_ATTACHMENT,
                )
            }),
            resolved: create(
                "overlay_target",
                1,
                // The composite reads it; tests copy it out.
                wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
            ),
        }
    }

    fn matches(&self, size: wgpu::Extent3d, format: wgpu::TextureFormat, samples: u32) -> bool {
        let descriptor = &self.resolved.descriptor;
        descriptor.size == size
            && descriptor.format == format
            && self
                .multisampled
                .as_ref()
                .map_or(1, |texture| texture.descriptor.sample_count)
                == samples
    }
}

/// Draws the enabled overlays into an overlay target after the
/// post-processing chain, then composites that target over the chain's
/// output.
///
/// The overlays share one render pass over the overlay target, cleared to
/// transparent, and the scene's depth, both at the scene's sample count, so
/// overlay edges and overlay-against-geometry intersections keep their
/// anti-aliasing. The pass resolves the target, and the composite blends the
/// resolve over the output as premultiplied alpha, `rgb + dst * (1 - a)`:
/// what alpha blending into a transparent target leaves there.
///
/// The target exists only while an overlay is enabled, so views without
/// overlays, such as the Game tab, the player and thumbnails, allocate
/// nothing for it.
pub struct OverlayCompositor {
    shader: Ref<Shader>,
    targets: Option<OverlayTargets>,
    bind_group: CachedBindGroup<(wgpu::BindGroupLayout, wgpu::TextureView)>,
}

impl OverlayCompositor {
    /// Shader asset the composite draws with.
    pub const SHADER: &'static str = "shaders/editor/overlay_composite";

    /// Format of the overlay target: display-linear color with half-float
    /// precision, the scene texture's format.
    pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

    /// Creates the compositor, or reports why its shader did not load.
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        Ok(Self {
            shader: SceneRendererError::load::<Shader>(context, Self::SHADER)?,
            targets: None,
            bind_group: Default::default(),
        })
    }

    /// The single-sample overlay target the last frame resolved into, or
    /// `None` while no overlay is enabled.
    pub fn target(&self) -> Option<&Texture> {
        self.targets.as_ref().map(|targets| &targets.resolved)
    }

    /// Frees the overlay target, as a resize needs.
    pub fn clear_targets(&mut self) {
        self.targets = None;
        self.bind_group.clear();
    }

    /// Records the overlays of `overlays` that are enabled, in order, into
    /// one pass over the overlay target and `depth`, then composites the
    /// target over `output`. Returns how many overlays recorded.
    ///
    /// `depth` is the scene's depth at `context.samples`; it is tested
    /// against and may be written. `output` is single-sample and blendable,
    /// and the overlay target takes its size. With no overlay enabled
    /// nothing is recorded and the overlay target is freed.
    pub fn run(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        overlays: &mut [&mut dyn OverlayPass],
        context: &OverlayContext,
        depth: &Texture,
        output: &Texture,
    ) -> usize {
        let mut enabled: Vec<&mut dyn OverlayPass> = overlays
            .iter_mut()
            .filter(|overlay| overlay.enabled(context))
            .map(|overlay| &mut **overlay)
            .collect();
        if enabled.is_empty() {
            self.clear_targets();
            return 0;
        }

        let size = output.descriptor.size;
        if !self
            .targets
            .as_ref()
            .is_some_and(|targets| targets.matches(size, context.format, context.samples))
        {
            self.clear_targets();
            self.targets = Some(OverlayTargets::new(
                context.render_context,
                size,
                context.format,
                context.samples,
            ));
        }
        let targets = self.targets.as_ref().expect("the targets were just made");

        for overlay in enabled.iter_mut() {
            overlay.prepare(context);
        }
        {
            let (view, resolve_target, store) = match &targets.multisampled {
                // Only the resolve is read, so the samples are discarded.
                Some(multisampled) => (
                    &multisampled.view,
                    Some(&targets.resolved.view),
                    wgpu::StoreOp::Discard,
                ),
                None => (&targets.resolved.view, None, wgpu::StoreOp::Store),
            };
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Overlays"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            for overlay in enabled.iter_mut() {
                overlay.record(context, &mut render_pass);
            }
        }
        self.composite(context.render_context, encoder, output);
        enabled.len()
    }

    /// Blends the resolved overlay target over `output` as premultiplied
    /// alpha.
    fn composite(
        &mut self,
        render_context: &RenderContext,
        encoder: &mut wgpu::CommandEncoder,
        output: &Texture,
    ) {
        let targets = self.targets.as_ref().expect("the overlays drew a target");
        let shader = self.shader.read();
        let Some(pipeline) = shader.pipeline(
            &PipelineOptions::fullscreen_builder()
                .target(wgpu::ColorTargetState {
                    format: output.descriptor.format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })
                .build(),
        ) else {
            return;
        };
        let layout = shader.bind_group_layouts[0].clone();
        drop(shader);
        let device = render_context.device();
        let overlay = &targets.resolved.view;
        let bind_group = self
            .bind_group
            .get_or_create((layout.clone(), overlay.clone()), || {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("overlay_composite_bind_group"),
                    layout: &layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(overlay),
                    }],
                })
            });
        let mut render_pass =
            RenderUtils::overlay_pass(encoder, &output.view, None, "Overlay Composite");
        render_pass.set_pipeline(&pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        RenderUtils::draw_fullscreen(&mut render_pass);
    }
}
