use crate::assets::texture::Texture;
use crate::assets::AssetId;
use crate::component::{ComponentParticleSystem, ParticleBlendMode, ParticleRenderInstance};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::buffer::{wgpu_buffer_init_desc, BufferLayout, ResizableBuffer};
use crate::render::{
    CachedBindGroup, PipelineOptions, RenderContext, RenderUtils, SceneRendererError, Shader,
    VertexLayout,
};
use crate::scene::{GameObject, Scene};
use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt;
use legion::{Entity, IntoQuery};
use nalgebra_glm::Vec3;
use std::collections::{HashMap, HashSet};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ParticleVertex {
    position: [f32; 2],
    uv: [f32; 2],
}

impl ParticleVertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2];
}

impl BufferLayout for ParticleVertex {
    const ATTRIBS: &'static [wgpu::VertexAttribute] = &Self::ATTRIBUTES;
}

impl ParticleRenderInstance {
    const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![2 => Float32x4, 3 => Float32x4];
}

impl BufferLayout for ParticleRenderInstance {
    const ATTRIBS: &'static [wgpu::VertexAttribute] = &Self::ATTRIBUTES;
}

#[derive(Default)]
struct ParticleSystemRenderState {
    render_buffer: Vec<ParticleRenderInstance>,
}

pub struct ParticleRenderer {
    shader: Ref<Shader>,
    default_texture: Ref<Texture>,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    instance_buffer: ResizableBuffer,
    particle_systems: HashMap<GameObject, ParticleSystemRenderState>,
    camera_bind_group: CachedBindGroup<(wgpu::BindGroupLayout, wgpu::Buffer)>,
    /// Texture bind groups of the textures drawn last frame, by texture id.
    texture_bind_groups: HashMap<AssetId, CachedBindGroup<TextureBindGroupKey>>,
}

type TextureBindGroupKey = (wgpu::BindGroupLayout, wgpu::TextureView, wgpu::Sampler);

impl ParticleRenderer {
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        let device = context.render_context.device();
        let shader = SceneRendererError::load::<Shader>(context, "shaders/particles")?;
        let default_texture = SceneRendererError::load::<Texture>(context, "textures/white")?;
        let vertices = [
            ParticleVertex {
                position: [-0.5, -0.5],
                uv: [0.0, 1.0],
            },
            ParticleVertex {
                position: [0.5, -0.5],
                uv: [1.0, 1.0],
            },
            ParticleVertex {
                position: [0.5, 0.5],
                uv: [1.0, 0.0],
            },
            ParticleVertex {
                position: [-0.5, 0.5],
                uv: [0.0, 0.0],
            },
        ];
        let indices = [0u16, 1, 2, 2, 3, 0];

        Ok(Self {
            shader,
            default_texture,
            vertex_buffer: device.create_buffer_init(&wgpu_buffer_init_desc(
                wgpu::BufferUsages::VERTEX,
                &vertices,
            )),
            index_buffer: device
                .create_buffer_init(&wgpu_buffer_init_desc(wgpu::BufferUsages::INDEX, &indices)),
            instance_buffer: ResizableBuffer::new(
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::VERTEX,
            ),
            particle_systems: HashMap::new(),
            camera_bind_group: Default::default(),
            texture_bind_groups: Default::default(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        render_context: &RenderContext,
        encoder: &mut wgpu::CommandEncoder,
        asset_context: &ReadOnlyAssetContext,
        scene: &Scene,
        camera_position: &Vec3,
        camera_uniform_buffer: &wgpu::Buffer,
        color_target: &Texture,
        depth_target: &Texture,
        samples: u32,
    ) {
        let device = render_context.device();
        let queue = render_context.queue();
        let color_format = color_target.descriptor.format;
        let depth_format = depth_target.descriptor.format;

        let camera_bind_group = {
            let shader = self.shader.read();
            let layout = &shader.bind_group_layouts[0];
            self.camera_bind_group
                .get_or_create((layout.clone(), camera_uniform_buffer.clone()), || {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("particle_camera_bind_group"),
                        layout,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: camera_uniform_buffer.as_entire_binding(),
                        }],
                    })
                })
                .clone()
        };
        let mut drawn_textures = HashSet::new();

        let mut query = <(Entity, &ComponentParticleSystem)>::query();
        for (entity, system) in query.iter(scene.world()) {
            let Some(game_object) = scene.game_object_from_entity(*entity) else {
                continue;
            };
            let emitter_transform = scene.world_transform(game_object);
            let particle_count = {
                let state = self.particle_systems.entry(game_object).or_default();
                system.fill_render_buffer(
                    &emitter_transform,
                    camera_position,
                    &mut state.render_buffer,
                );
                if state.render_buffer.is_empty() {
                    continue;
                }
                self.instance_buffer
                    .write_buffer(device, queue, &state.render_buffer, None);
                state.render_buffer.len() as u32
            };
            if particle_count == 0 {
                continue;
            }

            let texture = system
                .texture
                .get_ref(&asset_context.registries)
                .unwrap_or_else(|| self.default_texture.clone());
            let blend_mode = system.blend_mode;
            drawn_textures.insert(texture.id());
            let texture_bind_group = self.texture_bind_group(device, &texture);
            let pipeline = self.pipeline(blend_mode, samples, color_format, depth_format);

            let mut render_pass = RenderUtils::overlay_pass(
                encoder,
                &color_target.view,
                Some(&depth_target.view),
                "Particle Pass",
            );
            render_pass.set_pipeline(&pipeline);
            render_pass.set_bind_group(0, &camera_bind_group, &[]);
            render_pass.set_bind_group(1, &texture_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            render_pass.set_vertex_buffer(1, self.instance_buffer.get_wgpu_buffer().slice(..));
            render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            render_pass.draw_indexed(0..6, 0, 0..particle_count);
        }
        self.texture_bind_groups
            .retain(|id, _| drawn_textures.contains(id));
    }

    /// Returns the particle shader's pipeline for `blend_mode` and the given
    /// targets. The shader builds it on first use and drops it on hot reload.
    fn pipeline(
        &self,
        blend_mode: ParticleBlendMode,
        samples: u32,
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        self.shader.read().pipeline(&Self::pipeline_options(
            blend_mode,
            samples,
            color_format,
            depth_format,
        ))
    }

    fn pipeline_options(
        blend_mode: ParticleBlendMode,
        samples: u32,
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
    ) -> PipelineOptions {
        let mut depth_stencil = RenderUtils::depth_default(depth_format);
        depth_stencil.depth_write_enabled = false;
        let color_target = match blend_mode {
            ParticleBlendMode::Alpha => RenderUtils::color_alpha_blending(color_format),
            ParticleBlendMode::Additive => RenderUtils::color_additive_blending(color_format),
        };
        PipelineOptions::builder()
            .samples(samples)
            .cull_mode(None)
            .depth_stencil(Some(depth_stencil))
            .fragment_targets(vec![Some(color_target)])
            .vertex_layouts(vec![
                VertexLayout::of::<ParticleVertex>(wgpu::VertexStepMode::Vertex),
                VertexLayout::of::<ParticleRenderInstance>(wgpu::VertexStepMode::Instance),
            ])
            .build()
    }

    fn texture_bind_group(
        &mut self,
        device: &wgpu::Device,
        texture_ref: &Ref<Texture>,
    ) -> wgpu::BindGroup {
        let shader = self.shader.read();
        let texture = texture_ref.read();
        let layout = &shader.bind_group_layouts[1];
        let key = (
            layout.clone(),
            texture.view.clone(),
            texture.sampler.clone(),
        );
        self.texture_bind_groups
            .entry(texture_ref.id())
            .or_default()
            .get_or_create(key, || {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("particle_texture_bind_group"),
                    layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&texture.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&texture.sampler),
                        },
                    ],
                })
            })
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::ParticleRenderer;
    use crate::component::ParticleBlendMode;
    use crate::render::Shader;
    use crate::test_support::TestAssets;
    use egui_wgpu::wgpu::TextureFormat;

    #[test]
    fn particle_renderer_builds_a_pipeline_per_blend_mode() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let renderer =
            ParticleRenderer::new(&read_only_context).expect("particle renderer should load");

        let alpha = renderer.pipeline(
            ParticleBlendMode::Alpha,
            1,
            TextureFormat::Rgba16Float,
            TextureFormat::Depth32Float,
        );
        let additive = renderer.pipeline(
            ParticleBlendMode::Additive,
            1,
            TextureFormat::Rgba16Float,
            TextureFormat::Depth32Float,
        );
        let multisampled = renderer.pipeline(
            ParticleBlendMode::Alpha,
            4,
            TextureFormat::Rgba16Float,
            TextureFormat::Depth32Float,
        );

        assert_ne!(alpha, additive);
        assert_ne!(alpha, multisampled);
        assert_eq!(
            alpha,
            renderer.pipeline(
                ParticleBlendMode::Alpha,
                1,
                TextureFormat::Rgba16Float,
                TextureFormat::Depth32Float,
            )
        );
        assert_eq!(renderer.shader.read().pipeline_count(), 3);
    }

    #[test]
    fn particle_renderer_uses_the_reloaded_shader_pipeline() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let renderer =
            ParticleRenderer::new(&read_only_context).expect("particle renderer should load");
        let before = renderer.pipeline(
            ParticleBlendMode::Alpha,
            1,
            TextureFormat::Rgba16Float,
            TextureFormat::Depth32Float,
        );

        let registry = read_only_context.registries.assets.read();
        let id = registry
            .asset_id("shaders/particles")
            .expect("particle shader should have metadata");
        let reloaded = registry
            .reload_by_id::<Shader>(id)
            .expect("particle shader should reload");
        assert_eq!(reloaded.ptr_id(), renderer.shader.ptr_id());

        let after = renderer.pipeline(
            ParticleBlendMode::Alpha,
            1,
            TextureFormat::Rgba16Float,
            TextureFormat::Depth32Float,
        );
        assert_ne!(before, after);
        assert_eq!(
            after,
            reloaded
                .read()
                .pipeline(&ParticleRenderer::pipeline_options(
                    ParticleBlendMode::Alpha,
                    1,
                    TextureFormat::Rgba16Float,
                    TextureFormat::Depth32Float,
                ))
        );
        assert_eq!(reloaded.read().pipeline_count(), 1);
    }
}
