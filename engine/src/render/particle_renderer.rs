use crate::assets::texture::Texture;
use crate::component::{ComponentParticleSystem, Particle};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::buffer::{wgpu_buffer_init_desc, BufferLayout, ResizableBuffer};
use crate::render::{RenderUtils, Shader};
use crate::scene::{GameObject, Scene};
use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt;
use egui_wgpu::RenderState;
use nalgebra_glm::Vec3;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

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

impl Particle {
    const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![2 => Float32x4, 3 => Float32x4];
}

impl BufferLayout for Particle {
    const ATTRIBS: &'static [wgpu::VertexAttribute] = &Self::ATTRIBUTES;
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct PipelineSignature {
    samples: u32,
    color_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    shader_hash: u64,
}

pub struct ParticleRenderer {
    shader: Ref<Shader>,
    default_texture: Ref<Texture>,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    instance_buffer: ResizableBuffer,
    game_objects: Vec<GameObject>,
    pipeline: Option<wgpu::RenderPipeline>,
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
            game_objects: Vec::new(),
            pipeline: None,
            pipeline_signature: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        render_state: &RenderState,
        encoder: &mut wgpu::CommandEncoder,
        asset_context: &ReadOnlyAssetContext,
        scene: &mut Scene,
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
        let Some(pipeline) = self.pipeline.as_ref() else {
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

        self.game_objects.clear();
        self.game_objects.extend(scene.objects());
        let particle_system_marker = ComponentParticleSystem::default();

        for game_object in self.game_objects.iter().copied() {
            let emitter_transform = scene.world_transform(game_object);
            let Some(system) = (unsafe {
                scene
                    .get_component_ptr(game_object, &particle_system_marker)
                    .map(|ptr| &mut *(ptr as *mut ComponentParticleSystem))
            }) else {
                continue;
            };
            let particle_count = system.prepare_render_data(&emitter_transform, camera_position);
            if particle_count == 0 {
                continue;
            }
            self.instance_buffer.write_buffer(
                device,
                queue,
                system.render_particles(particle_count),
                None,
            );

            let texture = system
                .texture
                .get_ref(&asset_context.registries)
                .unwrap_or_else(|| self.default_texture.clone());
            let texture_bind_group = self.texture_bind_group(device, &texture);

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
            render_pass.draw_indexed(0..6, 0, 0..particle_count as u32);
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
        let shader_hash = Self::shader_hash(&shader);
        let signature = PipelineSignature {
            samples,
            color_format,
            depth_format,
            shader_hash,
        };
        if self.pipeline_signature == Some(signature) {
            return;
        }

        let mut depth_stencil = RenderUtils::depth_default(depth_format);
        depth_stencil.depth_write_enabled = false;

        self.pipeline = Some(
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("particle_pipeline"),
                layout: Some(&shader.pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader.shader,
                    entry_point: Some("vs_main"),
                    buffers: &[
                        ParticleVertex::layout(wgpu::VertexStepMode::Vertex),
                        Particle::layout(wgpu::VertexStepMode::Instance),
                    ],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader.shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(RenderUtils::color_alpha_blending(color_format))],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(depth_stencil),
                multisample: RenderUtils::multisample_default(samples),
                multiview: None,
                cache: None,
            }),
        );
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

    fn shader_hash(shader: &Shader) -> u64 {
        let mut hasher = DefaultHasher::new();
        shader.source.hash(&mut hasher);
        hasher.finish()
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
        assert!(renderer.pipeline.is_some());
    }
}
