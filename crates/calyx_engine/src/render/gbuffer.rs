//! The deferred G-buffer: the targets the geometry pass writes the scene's
//! PBR surfaces into, the pass itself, and the debug view that shows one
//! target in place of the lit image. The layout of each target is in
//! `docs/design/rendering/gbuffer.md`.

use crate::assets::texture::Texture;
use crate::assets::AssetId;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::asset_render_state::LockedAssetRenderState;
use crate::render::{
    CachedBindGroup, DrawBatch, MaterialBindGroups, PipelineOptions, RenderContext, RenderUtils,
    SceneRendererError, Shader,
};
use egui_wgpu::wgpu;
use std::sync::Arc;

/// A G-buffer target the scene view can show in place of the lit image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GBufferView {
    /// The base color, as the material's base color texture times its
    /// factor.
    Albedo,
    /// The view-space shading normal, each axis mapped from `[-1, 1]` to
    /// `[0, 1]`.
    Normals,
    /// Metallic in red, roughness in green, ambient occlusion in blue.
    Material,
    /// The linear HDR emission, compressed into `[0, 1]` with Reinhard.
    Emissive,
}

impl GBufferView {
    /// Every view, in the order the scene view lists them.
    pub const ALL: [GBufferView; 4] = [
        GBufferView::Albedo,
        GBufferView::Normals,
        GBufferView::Material,
        GBufferView::Emissive,
    ];

    /// The name the scene view shows for this view.
    pub fn label(self) -> &'static str {
        match self {
            GBufferView::Albedo => "Albedo",
            GBufferView::Normals => "Normals",
            GBufferView::Material => "Material",
            GBufferView::Emissive => "Emissive",
        }
    }

    fn index(self) -> u32 {
        match self {
            GBufferView::Albedo => 0,
            GBufferView::Normals => 1,
            GBufferView::Material => 2,
            GBufferView::Emissive => 3,
        }
    }
}

/// The G-buffer's targets, all single-sample at the scene's size.
pub struct GBufferTargets {
    /// Base color in RGB; alpha 1 where a surface was drawn, 0 elsewhere.
    pub albedo: Texture,
    /// The view-space shading normal, octahedral, in RG; the view-space
    /// geometric normal, octahedral, in BA.
    pub normals: Texture,
    /// Metallic in R, roughness in G, ambient occlusion in B.
    pub material: Texture,
    /// Linear HDR emission in RGB.
    pub emissive: Texture,
    /// The pass's own single-sample depth, above one sample only. At one
    /// sample the pass shares the scene depth.
    depth: Option<Texture>,
}

impl GBufferTargets {
    /// Format of [`Self::albedo`]: linear color stored as sRGB.
    pub const ALBEDO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
    /// Format of [`Self::normals`]: two octahedral normals at half float.
    pub const NORMALS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
    /// Format of [`Self::material`].
    pub const MATERIAL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
    /// Format of [`Self::emissive`]: linear HDR.
    pub const EMISSIVE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

    fn new(
        render_context: &Arc<RenderContext>,
        (width, height): (u32, u32),
        own_depth: bool,
    ) -> Self {
        let target = |label: &str, format: wgpu::TextureFormat| {
            Texture::new(
                render_context.clone(),
                &wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                },
                None,
                None,
                false,
            )
        };
        log::info!("Allocating the {width}x{height} G-buffer");
        Self {
            albedo: target("gbuffer_albedo", Self::ALBEDO_FORMAT),
            normals: target("gbuffer_normals", Self::NORMALS_FORMAT),
            material: target("gbuffer_material", Self::MATERIAL_FORMAT),
            emissive: target("gbuffer_emissive", Self::EMISSIVE_FORMAT),
            depth: own_depth.then(|| target("gbuffer_depth", wgpu::TextureFormat::Depth32Float)),
        }
    }

    /// The targets' size in pixels.
    pub fn size(&self) -> (u32, u32) {
        let size = self.albedo.descriptor.size;
        (size.width, size.height)
    }

    /// Whether the pass draws against its own depth rather than the scene's.
    pub fn has_own_depth(&self) -> bool {
        self.depth.is_some()
    }

    fn color_targets(&self) -> [&Texture; 4] {
        [&self.albedo, &self.normals, &self.material, &self.emissive]
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct GBufferViewUniform {
    view: u32,
    _padding: [u32; 3],
}

/// Draws the scene's PBR meshes into the G-buffer and shows a target in the
/// debug view. It owns the targets, which exist only while the pass runs.
pub(crate) struct GBufferRenderer {
    /// `shaders/mesh/gbuffer.wgsl`, the geometry pass.
    shader: Ref<Shader>,
    /// `shaders/deferred/gbuffer_view.wgsl`, the debug view.
    view_shader: Ref<Shader>,
    /// The PBR shader: only its draws enter the geometry pass, and their
    /// material bind groups, built for its group 3, bind as group 2.
    pbr_shader_id: AssetId,
    targets: Option<GBufferTargets>,
    camera_bind_group: CachedBindGroup<(wgpu::BindGroupLayout, wgpu::Buffer)>,
    view_uniform_buffer: wgpu::Buffer,
    view_bind_group: CachedBindGroup<(wgpu::BindGroupLayout, [wgpu::TextureView; 4])>,
    /// Instances the last [`Self::render`] drew.
    drawn_instances: u32,
}

impl GBufferRenderer {
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        let device = context.render_context.device();
        Ok(Self {
            shader: SceneRendererError::load::<Shader>(context, "shaders/mesh/gbuffer")?,
            view_shader: SceneRendererError::load::<Shader>(
                context,
                "shaders/deferred/gbuffer_view",
            )?,
            pbr_shader_id: SceneRendererError::load::<Shader>(context, "shaders/mesh/pbr")?.id(),
            targets: None,
            camera_bind_group: Default::default(),
            view_uniform_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("gbuffer_view_uniform"),
                size: size_of::<GBufferViewUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            view_bind_group: Default::default(),
            drawn_instances: 0,
        })
    }

    /// The targets, while the pass runs.
    pub fn targets(&self) -> Option<&GBufferTargets> {
        self.targets.as_ref()
    }

    /// Instances the last frame's geometry pass drew.
    pub fn drawn_instances(&self) -> u32 {
        self.drawn_instances
    }

    /// Frees the targets, on a frame the pass does not run.
    pub fn release(&mut self) {
        if self.targets.take().is_some() {
            log::info!("Releasing the G-buffer");
        }
        self.view_bind_group.clear();
        self.drawn_instances = 0;
    }

    /// Recreates the targets at the scene's new size, when they exist.
    pub fn resize(&mut self, render_context: &Arc<RenderContext>, size: (u32, u32)) {
        if let Some(targets) = &self.targets {
            if targets.size() != size {
                let own_depth = targets.has_own_depth();
                self.targets = Some(GBufferTargets::new(render_context, size, own_depth));
                self.view_bind_group.clear();
            }
        }
    }

    /// Draws every batch of `draw_list` that uses the PBR shader into the
    /// G-buffer, after the forward pass has drawn the scene into
    /// `scene_depth`. At one sample the pass tests against that depth
    /// without writing it, so only the surfaces the forward pass kept land
    /// in the G-buffer. Above one sample the scene depth cannot be attached
    /// to single-sample targets, so the pass draws against its own depth.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        render_context: &Arc<RenderContext>,
        encoder: &mut wgpu::CommandEncoder,
        assets: &LockedAssetRenderState,
        material_bind_groups: &MaterialBindGroups,
        draw_list: &[DrawBatch],
        camera_uniform_buffer: &wgpu::Buffer,
        scene_depth: &Texture,
        cull_mode: Option<wgpu::Face>,
    ) {
        let device = render_context.device();
        let size = (
            scene_depth.descriptor.size.width,
            scene_depth.descriptor.size.height,
        );
        let own_depth = scene_depth.descriptor.sample_count > 1;
        if self
            .targets
            .as_ref()
            .is_none_or(|targets| targets.size() != size || targets.has_own_depth() != own_depth)
        {
            self.targets = Some(GBufferTargets::new(render_context, size, own_depth));
            self.view_bind_group.clear();
        }
        let targets = self
            .targets
            .as_ref()
            .expect("the targets were just created");

        let depth_stencil = if own_depth {
            RenderUtils::depth_default(wgpu::TextureFormat::Depth32Float)
        } else {
            wgpu::DepthStencilState {
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                ..RenderUtils::depth_default(wgpu::TextureFormat::Depth32Float)
            }
        };
        let options = PipelineOptions::builder()
            .samples(1)
            .cull_mode(cull_mode)
            .fragment_targets(
                targets
                    .color_targets()
                    .map(|target| Some(RenderUtils::color_default(target.descriptor.format)))
                    .to_vec(),
            )
            .depth_stencil(Some(depth_stencil))
            .build();
        let shader = self.shader.read();
        let pipeline = shader.pipeline(&options);
        let layout = shader.bind_group_layouts[0].clone();
        drop(shader);
        let camera_bind_group = self
            .camera_bind_group
            .get_or_create((layout.clone(), camera_uniform_buffer.clone()), || {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("gbuffer_camera_bind_group"),
                    layout: &layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: camera_uniform_buffer.as_entire_binding(),
                    }],
                })
            })
            .clone();

        let color_attachments = targets.color_targets().map(|target| {
            Some(wgpu::RenderPassColorAttachment {
                view: &target.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })
        });
        let (depth_view, depth_load) = match &targets.depth {
            Some(depth) => (&depth.view, wgpu::LoadOp::Clear(1.0)),
            None => (&scene_depth.view, wgpu::LoadOp::Load),
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("G-Buffer"),
            color_attachments: &color_attachments,
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: depth_load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        self.drawn_instances = 0;
        let Some(pipeline) = pipeline else {
            return;
        };
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &camera_bind_group, &[]);
        // The pass tests `LessEqual` without writing depth, so of fragments
        // that tie in depth the last drawn lands, where the forward pass's
        // `Less` keeps the first drawn. Drawing the batches in reverse keeps
        // the forward winner across batches; the instances and triangles of
        // one batch share its material.
        for batch in draw_list
            .iter()
            .rev()
            .filter(|batch| batch.shader_id == self.pbr_shader_id)
        {
            let Some(material_group) = material_bind_groups
                .get(&batch.mat_id)
                .and_then(|groups| groups.get(&3))
            else {
                continue;
            };
            let Some(mesh) = assets.mesh(batch.mesh_id) else {
                continue;
            };
            let Some(instance_group) = mesh.instance_bind_group() else {
                continue;
            };
            pass.set_bind_group(1, instance_group, &[]);
            pass.set_bind_group(2, material_group, &[]);
            RenderUtils::bind_mesh_buffers(&mut pass, mesh);
            RenderUtils::draw_mesh_instanced(&mut pass, mesh, batch.instances.clone());
            self.drawn_instances += batch.instances.len() as u32;
        }
    }

    /// Draws `view` of the targets into `output`, over every pixel. Returns
    /// `false`, drawing nothing, when the pass has not run.
    pub fn render_view(
        &mut self,
        render_context: &RenderContext,
        encoder: &mut wgpu::CommandEncoder,
        view: GBufferView,
        output: &Texture,
    ) -> bool {
        let Some(targets) = &self.targets else {
            return false;
        };
        let device = render_context.device();
        render_context.queue().write_buffer(
            &self.view_uniform_buffer,
            0,
            bytemuck::bytes_of(&GBufferViewUniform {
                view: view.index(),
                _padding: Default::default(),
            }),
        );
        let shader = self.view_shader.read();
        let Some(pipeline) = shader.pipeline(
            &PipelineOptions::fullscreen_builder()
                .target(RenderUtils::color_default(output.descriptor.format))
                .build(),
        ) else {
            return false;
        };
        let layout = shader.bind_group_layouts[0].clone();
        drop(shader);
        let views = targets.color_targets().map(|target| target.view.clone());
        let uniform_buffer = &self.view_uniform_buffer;
        let bind_group =
            self.view_bind_group
                .get_or_create((layout.clone(), views.clone()), || {
                    let mut entries: Vec<wgpu::BindGroupEntry> = views
                        .iter()
                        .enumerate()
                        .map(|(binding, view)| wgpu::BindGroupEntry {
                            binding: binding as u32,
                            resource: wgpu::BindingResource::TextureView(view),
                        })
                        .collect();
                    entries.push(wgpu::BindGroupEntry {
                        binding: views.len() as u32,
                        resource: uniform_buffer.as_entire_binding(),
                    });
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("gbuffer_view_bind_group"),
                        layout: &layout,
                        entries: &entries,
                    })
                });
        let mut pass = RenderUtils::fullscreen_pass(encoder, &output.view, "G-Buffer View");
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        RenderUtils::draw_fullscreen(&mut pass);
        true
    }
}
