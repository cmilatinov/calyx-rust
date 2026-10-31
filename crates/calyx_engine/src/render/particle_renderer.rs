use crate::assets::texture::Texture;
use crate::assets::AssetId;
use crate::component::{ComponentParticleSystem, ParticleBlendMode, ParticleRenderInstance};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::buffer::{wgpu_buffer_init_desc, BufferLayout};
use crate::render::{
    buffer_entry, enabled_system, workgroups, CachedBindGroup, ComputeKernel, GpuCurveRange,
    GpuGradientRange, ParticleSimulation, PipelineOptions, RenderContext, RenderUtils,
    SceneRendererError, Shader, SystemKeys, VertexLayout, PARTICLE_KERNELS,
};
use crate::scene::{GameObject, Scene, SceneId};
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

/// `RenderParams` of `particles_simulate.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct RenderParams {
    emitter: [[f32; 4]; 4],
    camera_position: [f32; 3],
    local_space: u32,
    slot_count: u32,
    sort_count: u32,
    _padding: [u32; 2],
    size_curve: GpuCurveRange,
    color_gradient: GpuGradientRange,
}

/// `SortParams` of `particles_simulate.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SortParams {
    j: u32,
    k: u32,
    _padding: [u32; 2],
}

/// The `(j, k)` of each compare-and-swap pass of a bitonic sort over `count`
/// keys, a power of two, in order. The passes of a smaller count are a
/// prefix of a larger count's.
fn sort_passes(count: u32) -> Vec<(u32, u32)> {
    let mut passes = Vec::new();
    let mut k = 2;
    while k <= count {
        let mut j = k / 2;
        while j > 0 {
            passes.push((j, k));
            j /= 2;
        }
        k *= 2;
    }
    passes
}

/// The evaluate, sort and gather kernels.
struct ViewKernels {
    evaluate: ComputeKernel,
    sort: ComputeKernel,
    gather: ComputeKernel,
}

/// Parameters of every sort pass, one per uniform-aligned entry, shared by
/// every system the view draws.
struct SortPassParams {
    /// Sort size the entries cover.
    count: u32,
    stride: u64,
    buffer: wgpu::Buffer,
}

impl SortPassParams {
    fn new(device: &wgpu::Device, count: u32) -> Self {
        let stride = (device.limits().min_uniform_buffer_offset_alignment as u64)
            .max(size_of::<SortParams>() as u64);
        let passes = sort_passes(count);
        let mut contents = vec![0u8; (passes.len().max(1) as u64 * stride) as usize];
        for (index, (j, k)) in passes.into_iter().enumerate() {
            let offset = index * stride as usize;
            contents[offset..offset + size_of::<SortParams>()].copy_from_slice(bytemuck::bytes_of(
                &SortParams {
                    j,
                    k,
                    _padding: [0; 2],
                },
            ));
        }
        Self {
            count,
            stride,
            buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("particle_sort_params"),
                contents: &contents,
                usage: wgpu::BufferUsages::UNIFORM,
            }),
        }
    }
}

type ViewBindGroup = CachedBindGroup<(wgpu::BindGroupLayout, wgpu::Buffer)>;
type EvaluateBindGroup = CachedBindGroup<(
    wgpu::BindGroupLayout,
    wgpu::Buffer,
    wgpu::Buffer,
    wgpu::Buffer,
)>;

/// What one view keeps for one simulated system: the buffers its particles
/// are evaluated, sorted and gathered through, and the bind groups over
/// them.
struct ViewSystem {
    slot_count: u32,
    /// Keys sorted, a power of two at least `slot_count`.
    sort_count: u32,
    params: wgpu::Buffer,
    keys: wgpu::Buffer,
    evaluated: wgpu::Buffer,
    /// The sorted instances the draw reads.
    instances: wgpu::Buffer,
    evaluate_group: EvaluateBindGroup,
    /// One bind group per sort pass.
    sort_groups: Vec<ViewBindGroup>,
    gather_group: CachedBindGroup<wgpu::BindGroupLayout>,
}

impl ViewSystem {
    fn new(device: &wgpu::Device, slot_count: u32) -> Self {
        let sort_count = slot_count.max(1).next_power_of_two();
        let storage = |label, size: usize, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size as u64,
                usage: wgpu::BufferUsages::STORAGE | usage,
                mapped_at_creation: false,
            })
        };
        let slots = slot_count.max(1) as usize;
        Self {
            slot_count,
            sort_count,
            params: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("particle_render_params"),
                size: size_of::<RenderParams>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            keys: storage(
                "particle_sort_keys",
                sort_count as usize * 8,
                wgpu::BufferUsages::empty(),
            ),
            evaluated: storage(
                "particle_evaluated",
                slots * size_of::<ParticleRenderInstance>(),
                wgpu::BufferUsages::empty(),
            ),
            instances: storage(
                "particle_instances",
                slots * size_of::<ParticleRenderInstance>(),
                wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_SRC,
            ),
            evaluate_group: Default::default(),
            sort_groups: Vec::new(),
            gather_group: Default::default(),
        }
    }

    /// Records evaluate, every sort pass, then gather.
    fn record(
        &mut self,
        device: &wgpu::Device,
        kernels: &ViewKernels,
        sort_params: &SortPassParams,
        particles: &wgpu::Buffer,
        keys: &SystemKeys,
        pass: &mut wgpu::ComputePass,
    ) {
        let layout = &kernels.evaluate.bind_group_layouts[0];
        let group = self.evaluate_group.get_or_create(
            (
                layout.clone(),
                particles.clone(),
                keys.curve_keys.clone(),
                keys.gradient_keys.clone(),
            ),
            || {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("particle_evaluate_bind_group"),
                    layout,
                    entries: &[
                        buffer_entry(4, &self.params),
                        buffer_entry(5, particles),
                        buffer_entry(6, &self.keys),
                        buffer_entry(7, &self.evaluated),
                        buffer_entry(10, &keys.curve_keys),
                        buffer_entry(11, &keys.gradient_keys),
                    ],
                })
            },
        );
        let (x, y) = workgroups(self.sort_count);
        pass.set_pipeline(&kernels.evaluate.pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.dispatch_workgroups(x, y, 1);

        let passes = sort_passes(self.sort_count).len();
        self.sort_groups.resize_with(passes, Default::default);
        let layout = &kernels.sort.bind_group_layouts[0];
        pass.set_pipeline(&kernels.sort.pipeline);
        for (index, group) in self.sort_groups.iter_mut().enumerate() {
            let group = group.get_or_create((layout.clone(), sort_params.buffer.clone()), || {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("particle_sort_bind_group"),
                    layout,
                    entries: &[
                        buffer_entry(4, &self.params),
                        buffer_entry(6, &self.keys),
                        wgpu::BindGroupEntry {
                            binding: 9,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &sort_params.buffer,
                                offset: index as u64 * sort_params.stride,
                                size: wgpu::BufferSize::new(size_of::<SortParams>() as u64),
                            }),
                        },
                    ],
                })
            });
            pass.set_bind_group(0, group, &[]);
            pass.dispatch_workgroups(x, y, 1);
        }

        let layout = &kernels.gather.bind_group_layouts[0];
        let group = self.gather_group.get_or_create(layout.clone(), || {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("particle_gather_bind_group"),
                layout,
                entries: &[
                    buffer_entry(4, &self.params),
                    buffer_entry(6, &self.keys),
                    buffer_entry(7, &self.evaluated),
                    buffer_entry(8, &self.instances),
                ],
            })
        });
        let (x, y) = workgroups(self.slot_count);
        pass.set_pipeline(&kernels.gather.pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.dispatch_workgroups(x, y, 1);
    }
}

/// One system's draw this frame.
struct ParticleDraw {
    game_object: GameObject,
    /// The system's curve and gradient keys.
    keys: SystemKeys,
    texture: Ref<Texture>,
    blend_mode: ParticleBlendMode,
    live_count: u32,
}

/// Draws the particles of a [`ParticleSimulation`] for one view.
///
/// It keeps only per-view state: for each simulated system the buffers its
/// particles are evaluated, sorted back to front and gathered through, kept
/// until the system leaves the simulation.
pub struct ParticleRenderer {
    shader: Ref<Shader>,
    kernels: Ref<Shader>,
    default_texture: Ref<Texture>,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    /// The scene of the simulation the per-view state belongs to.
    simulation_scene: Option<SceneId>,
    /// Per-view state of each simulated system drawn so far.
    particle_systems: HashMap<GameObject, ViewSystem>,
    sort_params: Option<SortPassParams>,
    camera_bind_group: CachedBindGroup<(wgpu::BindGroupLayout, wgpu::Buffer)>,
    /// Texture bind groups of the textures drawn last frame, by texture id.
    texture_bind_groups: HashMap<AssetId, CachedBindGroup<TextureBindGroupKey>>,
}

type TextureBindGroupKey = (wgpu::BindGroupLayout, wgpu::TextureView, wgpu::Sampler);

impl ParticleRenderer {
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        let device = context.render_context.device();
        let shader = SceneRendererError::load::<Shader>(context, "shaders/particles/particles")?;
        let kernels = SceneRendererError::load::<Shader>(context, PARTICLE_KERNELS)?;
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
            kernels,
            default_texture,
            vertex_buffer: device.create_buffer_init(&wgpu_buffer_init_desc(
                wgpu::BufferUsages::VERTEX,
                &vertices,
            )),
            index_buffer: device
                .create_buffer_init(&wgpu_buffer_init_desc(wgpu::BufferUsages::INDEX, &indices)),
            simulation_scene: None,
            particle_systems: HashMap::new(),
            sort_params: None,
            camera_bind_group: Default::default(),
            texture_bind_groups: Default::default(),
        })
    }

    /// Draws `simulation`'s particles when it simulates `scene`, sorted back
    /// to front from `camera_position`. Draws nothing for another scene or
    /// without a simulation.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        render_context: &RenderContext,
        encoder: &mut wgpu::CommandEncoder,
        asset_context: &ReadOnlyAssetContext,
        scene: &Scene,
        simulation: Option<&ParticleSimulation>,
        camera_position: &Vec3,
        camera_uniform_buffer: &wgpu::Buffer,
        color_target: &Texture,
        depth_target: &Texture,
        samples: u32,
    ) {
        let Some(simulation) = simulation else {
            return;
        };
        // Per-view state lives as long as its system in the simulation.
        if self.simulation_scene != simulation.scene_id() {
            self.particle_systems.clear();
            self.simulation_scene = simulation.scene_id();
        }
        self.particle_systems
            .retain(|game_object, _| simulation.system(*game_object).is_some());
        if simulation.scene_id() != Some(scene.id()) {
            return;
        }
        let Some(kernels) = self.view_kernels() else {
            return;
        };
        let device = render_context.device();
        let queue = render_context.queue();

        let mut draws = Vec::new();
        let mut query = <(Entity, &ComponentParticleSystem)>::query();
        for (entity, system) in query.iter(scene.world()) {
            let Some(game_object) = enabled_system(scene, *entity) else {
                continue;
            };
            let Some(simulated) = simulation.system(game_object) else {
                continue;
            };
            let live_count = simulated.live_count();
            if live_count == 0 {
                continue;
            }
            let slot_count = simulated.capacity();
            let view = self
                .particle_systems
                .entry(game_object)
                .or_insert_with(|| ViewSystem::new(device, slot_count));
            if view.slot_count != slot_count {
                *view = ViewSystem::new(device, slot_count);
            }
            let keys = simulated.keys().clone();
            queue.write_buffer(
                &view.params,
                0,
                bytemuck::bytes_of(&RenderParams {
                    emitter: scene.world_transform(game_object).matrix().into(),
                    camera_position: (*camera_position).into(),
                    local_space: system.local_space as u32,
                    slot_count,
                    sort_count: view.sort_count,
                    _padding: [0; 2],
                    size_curve: keys.size_curve,
                    color_gradient: keys.color_gradient,
                }),
            );
            let texture = system
                .rendering
                .texture
                .get_ref(&asset_context.registries)
                .unwrap_or_else(|| self.default_texture.clone());
            draws.push(ParticleDraw {
                game_object,
                keys,
                texture,
                blend_mode: system.rendering.blend_mode,
                live_count,
            });
        }
        if draws.is_empty() {
            return;
        }

        let sort_count = draws
            .iter()
            .map(|draw| self.particle_systems[&draw.game_object].sort_count)
            .max()
            .unwrap_or(1);
        if self
            .sort_params
            .as_ref()
            .is_none_or(|params| params.count < sort_count)
        {
            self.sort_params = Some(SortPassParams::new(device, sort_count));
        }
        let sort_params = self.sort_params.as_ref().expect("just created");
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("particle_evaluate"),
                timestamp_writes: None,
            });
            for draw in &draws {
                let simulated = simulation
                    .system(draw.game_object)
                    .expect("drawn systems are simulated");
                self.particle_systems
                    .get_mut(&draw.game_object)
                    .expect("drawn systems have view state")
                    .record(
                        device,
                        &kernels,
                        sort_params,
                        simulated.particles(),
                        &draw.keys,
                        &mut pass,
                    );
            }
        }

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
        let color_format = color_target.descriptor.format;
        let depth_format = depth_target.descriptor.format;
        let mut drawn_textures = HashSet::new();
        for draw in draws {
            drawn_textures.insert(draw.texture.id());
            let texture_bind_group = self.texture_bind_group(device, &draw.texture);
            let Some(pipeline) =
                self.pipeline(draw.blend_mode, samples, color_format, depth_format)
            else {
                continue;
            };
            let instances = &self.particle_systems[&draw.game_object].instances;

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
            render_pass.set_vertex_buffer(1, instances.slice(..));
            render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            render_pass.draw_indexed(0..6, 0, 0..draw.live_count);
        }
        self.texture_bind_groups
            .retain(|id, _| drawn_textures.contains(id));
    }

    fn view_kernels(&self) -> Option<ViewKernels> {
        let shader = self.kernels.read();
        Some(ViewKernels {
            evaluate: shader.compute_pipeline("evaluate_main")?,
            sort: shader.compute_pipeline("sort_main")?,
            gather: shader.compute_pipeline("gather_main")?,
        })
    }

    /// The game objects whose per-view state the renderer keeps.
    #[cfg(test)]
    pub(crate) fn tracked_systems(&self) -> HashSet<GameObject> {
        self.particle_systems.keys().copied().collect()
    }

    /// Reads back the instances drawn for `game_object` last frame, in draw
    /// order.
    #[cfg(test)]
    pub(crate) fn read_instances(
        &self,
        render_context: &RenderContext,
        game_object: GameObject,
    ) -> Vec<ParticleRenderInstance> {
        let view = &self.particle_systems[&game_object];
        let mut instances: Vec<ParticleRenderInstance> =
            crate::render::read_buffer(render_context, &view.instances);
        instances.truncate(view.slot_count as usize);
        instances
    }

    /// Returns the particle shader's pipeline for `blend_mode` and the given
    /// targets, or `None` when wgpu rejects it. The shader builds it on first use
    /// and drops it on hot reload.
    fn pipeline(
        &self,
        blend_mode: ParticleBlendMode,
        samples: u32,
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
    ) -> Option<wgpu::RenderPipeline> {
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
            .asset_id("shaders/particles/particles")
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
