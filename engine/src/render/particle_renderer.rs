use crate::assets::texture::Texture;
use crate::component::{ComponentParticleSystem, ParticleBlendMode, ParticleRenderInstance};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::buffer::{wgpu_buffer_init_desc, BufferLayout, ResizableBuffer};
use crate::render::{RenderUtils, Shader};
use crate::scene::{GameObject, Scene};
use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt;
use egui_wgpu::RenderState;
use legion::{Entity, IntoQuery};
use nalgebra_glm::Vec3;
use std::collections::HashMap;

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

#[derive(Clone, Copy, PartialEq, Eq)]
struct PipelineSignature {
    samples: u32,
    color_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
}

struct ParticlePipelines {
    alpha: wgpu::RenderPipeline,
    additive: wgpu::RenderPipeline,
}

pub struct ParticleRenderer {
    shader: Ref<Shader>,
    default_texture: Ref<Texture>,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    instance_buffer: ResizableBuffer,
    particle_systems: HashMap<GameObject, ParticleSystemRenderState>,
    pipelines: Option<ParticlePipelines>,
    pipeline_signature: Option<PipelineSignature>,
}

impl ParticleRenderer {
    pub fn new(context: &ReadOnlyAssetContext) -> Self {
        let device = context.render_context.device();
        let shader = context
            .registries
            .assets
            .read()
            .load::<Shader>("shaders/particles")
            .expect("missing particle shader");
        let default_texture = context
            .registries
            .assets
            .read()
            .load::<Texture>("textures/white")
            .expect("missing white texture");
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

        Self {
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
            pipelines: None,
            pipeline_signature: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        render_state: &RenderState,
        encoder: &mut wgpu::CommandEncoder,
        asset_context: &ReadOnlyAssetContext,
        scene: &Scene,
        camera_position: &Vec3,
        camera_uniform_buffer: &wgpu::Buffer,
        color_target: &Texture,
        depth_target: &Texture,
        samples: u32,
    ) {
        let device = &render_state.device;
        let queue = &render_state.queue;
        self.ensure_pipeline(
            device,
            samples,
            color_target.descriptor.format,
            depth_target.descriptor.format,
        );
        let Some(pipelines) = self.pipelines.as_ref() else {
            return;
        };

        let camera_bind_group = {
            let shader = self.shader.read();
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("particle_camera_bind_group"),
                layout: &shader.bind_group_layouts[0],
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_uniform_buffer.as_entire_binding(),
                }],
            })
        };

        let mut query = <(Entity, &ComponentParticleSystem)>::query();
        for (entity, system) in query.iter(&scene.world) {
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
            let texture_bind_group = self.texture_bind_group(device, &texture);
            let pipeline = match blend_mode {
                ParticleBlendMode::Alpha => &pipelines.alpha,
                ParticleBlendMode::Additive => &pipelines.additive,
            };

            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Particle Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_target.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_target.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            render_pass.set_pipeline(pipeline);
            render_pass.set_bind_group(0, &camera_bind_group, &[]);
            render_pass.set_bind_group(1, &texture_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            render_pass.set_vertex_buffer(1, self.instance_buffer.get_wgpu_buffer().slice(..));
            render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            render_pass.draw_indexed(0..6, 0, 0..particle_count);
        }
    }

    fn ensure_pipeline(
        &mut self,
        device: &wgpu::Device,
        samples: u32,
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
    ) {
        let shader = self.shader.read();
        let signature = PipelineSignature {
            samples,
            color_format,
            depth_format,
        };
        if self.pipeline_signature == Some(signature) {
            return;
        }

        let mut depth_stencil = RenderUtils::depth_default(depth_format);
        depth_stencil.depth_write_enabled = false;

        let create_pipeline = |label: &str, target: wgpu::ColorTargetState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&shader.pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader.shader,
                    entry_point: Some("vs_main"),
                    buffers: &[
                        ParticleVertex::layout(wgpu::VertexStepMode::Vertex),
                        ParticleRenderInstance::layout(wgpu::VertexStepMode::Instance),
                    ],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader.shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(target)],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(depth_stencil.clone()),
                multisample: RenderUtils::multisample_default(samples),
                multiview: None,
                cache: None,
            })
        };

        self.pipelines = Some(ParticlePipelines {
            alpha: create_pipeline(
                "particle_pipeline_alpha",
                RenderUtils::color_alpha_blending(color_format),
            ),
            additive: create_pipeline(
                "particle_pipeline_additive",
                RenderUtils::color_additive_blending(color_format),
            ),
        });
        self.pipeline_signature = Some(signature);
    }

    fn texture_bind_group(&self, device: &wgpu::Device, texture: &Ref<Texture>) -> wgpu::BindGroup {
        let shader = self.shader.read();
        let texture = texture.read();
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("particle_texture_bind_group"),
            layout: &shader.bind_group_layouts[1],
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
    }
}

#[cfg(test)]
mod tests {
    use super::ParticleRenderer;
    use crate::test_utils::test_asset_context_with_assets;
    use egui_wgpu::wgpu::TextureFormat;
    use std::path::PathBuf;

    fn assets_path() -> PathBuf {
        let assets_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets");
        dunce::canonicalize(assets_path).expect("assets dir not found")
    }

    #[test]
    fn particle_renderer_loads_shader_and_builds_pipeline() {
        let context = test_asset_context_with_assets(vec![assets_path()]);
        let read_only_context = context.lock_read();
        let mut renderer = ParticleRenderer::new(&read_only_context);
        let device = read_only_context.render_context.device();
        renderer.ensure_pipeline(
            device,
            1,
            TextureFormat::Rgba16Float,
            TextureFormat::Depth32Float,
        );
        assert!(renderer.pipelines.is_some());
    }
}
