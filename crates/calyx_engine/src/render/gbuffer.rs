//! The deferred G-buffer: the targets the geometry pass writes the scene's
//! PBR surfaces into, the pass itself, and the debug view that shows one
//! target in place of the lit image. The layout of each target is in
//! `docs/design/rendering/gbuffer.md`.

use crate::assets::texture::Texture;
use crate::assets::AssetId;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::profiler::PassScope;
use crate::render::asset_render_state::LockedAssetRenderState;
use crate::render::water::WATER_SHADER;
use crate::render::{
    CachedBindGroup, DrawBatch, MaterialBindGroups, PipelineOptions, RenderContext, RenderUtils,
    SceneRendererError, Shader, FIRST_MATERIAL_GROUP,
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

    /// The pass's own depth, above one sample only.
    pub(crate) fn own_depth(&self) -> Option<&Texture> {
        self.depth.as_ref()
    }

    pub(crate) fn color_targets(&self) -> [&Texture; 4] {
        [&self.albedo, &self.normals, &self.material, &self.emissive]
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct GBufferViewUniform {
    view: u32,
    _padding: [u32; 3],
}

/// One kind of surface the geometry pass draws: the draws whose material
/// shader is `material_shader`, drawn with `shader`, which binds the
/// material's group 3 bind group as its group 2.
struct GeometryShader {
    shader: Ref<Shader>,
    material_shader: AssetId,
}

/// Draws the scene's PBR meshes, water and blended ground into the
/// G-buffer and shows a target in the debug view. It owns the targets,
/// created on the first frame.
pub(crate) struct GBufferRenderer {
    /// The PBR surfaces, `shaders/mesh/gbuffer.wgsl` over the PBR shader's
    /// draws, then the water, `shaders/mesh/water_gbuffer.wgsl` over the
    /// water shader's, then the blended ground,
    /// `shaders/mesh/ground_gbuffer.wgsl` over the ground shader's. Only
    /// these draws enter the geometry pass.
    geometry: [GeometryShader; 3],
    /// `shaders/deferred/gbuffer_view.wgsl`, the debug view.
    view_shader: Ref<Shader>,
    targets: Option<GBufferTargets>,
    /// The camera's bind group for each of [`Self::geometry`]'s shaders,
    /// built for its own group 0.
    camera_bind_groups: [CachedBindGroup<(wgpu::BindGroupLayout, wgpu::Buffer)>; 3],
    view_uniform_buffer: wgpu::Buffer,
    view_bind_group: CachedBindGroup<(wgpu::BindGroupLayout, [wgpu::TextureView; 4])>,
    /// Instances the last [`Self::render`] drew.
    drawn_instances: u32,
}

impl GBufferRenderer {
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        let device = context.render_context.device();
        let geometry = |shader: &str, material_shader: &str| {
            Ok::<_, SceneRendererError>(GeometryShader {
                shader: SceneRendererError::load::<Shader>(context, shader)?,
                material_shader: SceneRendererError::load::<Shader>(context, material_shader)?.id(),
            })
        };
        Ok(Self {
            geometry: [
                geometry("shaders/mesh/gbuffer", "shaders/mesh/pbr")?,
                geometry("shaders/mesh/water_gbuffer", WATER_SHADER)?,
                geometry("shaders/mesh/ground_gbuffer", "shaders/mesh/ground")?,
            ],
            view_shader: SceneRendererError::load::<Shader>(
                context,
                "shaders/deferred/gbuffer_view",
            )?,
            targets: None,
            camera_bind_groups: Default::default(),
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

    /// The targets, once the first frame has drawn them.
    pub fn targets(&self) -> Option<&GBufferTargets> {
        self.targets.as_ref()
    }

    /// The PBR shader, whose draws the geometry pass draws as PBR surfaces.
    pub(crate) fn pbr_shader_id(&self) -> AssetId {
        self.geometry[0].material_shader
    }

    /// The material shaders whose draws the geometry pass draws: the PBR,
    /// the water and the ground shader. The forward pass draws the others.
    pub(crate) fn deferred_shaders(&self) -> [AssetId; 3] {
        self.geometry
            .each_ref()
            .map(|geometry| geometry.material_shader)
    }

    /// Instances the last frame's geometry pass drew.
    pub fn drawn_instances(&self) -> u32 {
        self.drawn_instances
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

    /// Draws every batch of `draw_list` that uses the PBR, water or ground
    /// shader into the G-buffer, first in the frame, and draws the depth itself: it clears
    /// it and tests `Less` in the draw order, so of fragments that tie in
    /// depth the first drawn is kept, as the forward-only draws that follow
    /// keep it. The depth is `scene_depth` at one sample and the targets'
    /// own single-sample depth above one, where the multisampled scene depth
    /// cannot be attached beside single-sample targets; the lighting pass
    /// then copies it into the scene depth.
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

        let depth_stencil = RenderUtils::depth_default(wgpu::TextureFormat::Depth32Float);
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
        // A hot reload of a geometry shader or its material shader can leave
        // group 2 apart from the materials' group until the other follows;
        // its materials are skipped meanwhile.
        let passes: Vec<_> = self
            .geometry
            .iter()
            .zip(&mut self.camera_bind_groups)
            .map(|(geometry, camera_bind_group)| {
                let shader = geometry.shader.read();
                let binds_materials =
                    assets
                        .shader(geometry.material_shader)
                        .is_some_and(|material_shader| {
                            shader.shares_material_group(2, material_shader, FIRST_MATERIAL_GROUP)
                        });
                let pipeline = shader.pipeline(&options).filter(|_| binds_materials);
                let layout = shader.bind_group_layouts[0].clone();
                let camera = camera_bind_group
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
                (
                    geometry.material_shader,
                    pipeline,
                    camera,
                    shader.name.clone(),
                )
            })
            .collect();

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
        let depth_view = match &targets.depth {
            Some(depth) => &depth.view,
            None => &scene_depth.view,
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("G-Buffer"),
            color_attachments: &color_attachments,
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        self.drawn_instances = 0;
        for (material_shader, pipeline, camera_bind_group, name) in &passes {
            let Some(pipeline) = pipeline else {
                continue;
            };
            let timed = PassScope::begin(&mut pass, || name.clone());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, camera_bind_group, &[]);
            // The draw list is sorted by shader, material, then mesh: bind
            // each only when it changes.
            let mut bound_material = None;
            let mut bound_mesh = None;
            for batch in draw_list
                .iter()
                .filter(|batch| batch.shader_id == *material_shader)
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
                if bound_mesh != Some(batch.mesh_id) {
                    pass.set_bind_group(1, instance_group, &[]);
                    RenderUtils::bind_mesh_buffers(&mut pass, mesh);
                    bound_mesh = Some(batch.mesh_id);
                }
                if bound_material != Some(batch.mat_id) {
                    pass.set_bind_group(2, material_group, &[]);
                    bound_material = Some(batch.mat_id);
                }
                RenderUtils::draw_mesh_instanced(&mut pass, mesh, batch.instances.clone());
                self.drawn_instances += batch.instances.len() as u32;
            }
            timed.end(&mut pass);
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
