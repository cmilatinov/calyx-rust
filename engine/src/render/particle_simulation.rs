//! The GPU particle simulation shared by every view of a scene.

use crate::component::{ComponentParticleSystem, EmitterState, Particle, ParticleSpawn};
use crate::core::Ref;
use crate::math::Curve;
use crate::render::buffer::ResizableBuffer;
use crate::render::{
    CachedBindGroup, ComputeKernel, GpuCurveKeys, GpuCurveRange, RenderContext, Shader,
};
use crate::scene::{GameObject, Scene, SceneId};
use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt;
use legion::{Entity, IntoQuery};
use std::collections::{HashMap, HashSet};

/// Registry name of the shader holding every particle kernel.
pub(crate) const PARTICLE_KERNELS: &str = "shaders/particles_simulate";
/// Threads per workgroup of every particle kernel.
const WORKGROUP_SIZE: u32 = 64;
/// Most workgroups one dispatch dimension takes.
const MAX_WORKGROUPS_PER_DIMENSION: u32 = 65535;

/// Workgroup counts that run at least `threads` threads of a particle
/// kernel, spread over two dimensions past the per-dimension limit.
pub(crate) fn workgroups(threads: u32) -> (u32, u32) {
    let groups = threads.div_ceil(WORKGROUP_SIZE).max(1);
    if groups <= MAX_WORKGROUPS_PER_DIMENSION {
        (groups, 1)
    } else {
        (
            MAX_WORKGROUPS_PER_DIMENSION,
            groups.div_ceil(MAX_WORKGROUPS_PER_DIMENSION),
        )
    }
}

/// `SimulationParams` of `particles_simulate.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SimulationParams {
    slot_count: u32,
    step_count: u32,
    spawn_count: u32,
    _padding: u32,
}

/// The update and scatter kernels.
struct SimulationKernels {
    update: ComputeKernel,
    scatter: ComputeKernel,
}

/// The curves of a system the evaluate kernel reads, in the order their
/// ranges are packed: the size curve, then the red, green, blue and alpha
/// curves.
fn system_curves(component: &ComponentParticleSystem) -> [&Curve; 5] {
    let [red, green, blue, alpha] = component.color.channels();
    [&component.size.curve, red, green, blue, alpha]
}

/// The GPU copy of a system's size and color curves.
struct SystemCurves {
    /// The curves the buffer holds, to upload again only when they change.
    curves: Vec<Curve>,
    ranges: [GpuCurveRange; 5],
    keys: wgpu::Buffer,
    /// Times the keys were uploaded.
    #[cfg(test)]
    uploads: u32,
}

impl SystemCurves {
    fn new(device: &wgpu::Device, component: &ComponentParticleSystem) -> Self {
        let curves = system_curves(component);
        let (keys, ranges) = GpuCurveKeys::pack(curves);
        Self {
            curves: curves.into_iter().cloned().collect(),
            ranges: ranges.try_into().expect("one range per curve"),
            keys: Self::key_buffer(device, &keys),
            #[cfg(test)]
            uploads: 1,
        }
    }

    fn key_buffer(device: &wgpu::Device, keys: &GpuCurveKeys) -> wgpu::Buffer {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("particle_curve_keys"),
            contents: keys.contents(),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        })
    }

    /// Uploads `component`'s curves when they differ from the ones held.
    fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        component: &ComponentParticleSystem,
    ) {
        let curves = system_curves(component);
        if self.curves.iter().eq(curves) {
            return;
        }
        let (keys, ranges) = GpuCurveKeys::pack(curves);
        let contents = keys.contents();
        if contents.len() as u64 <= self.keys.size() {
            queue.write_buffer(&self.keys, 0, contents);
        } else {
            self.keys = Self::key_buffer(device, &keys);
        }
        self.ranges = ranges.try_into().expect("one range per curve");
        self.curves = curves.into_iter().cloned().collect();
        #[cfg(test)]
        {
            self.uploads += 1;
        }
    }
}

/// One particle system as the simulation runs it: its CPU emitter state and
/// the GPU buffers of its particle slots and curves.
pub(crate) struct SimulatedSystem {
    emitter: EmitterState,
    /// [`ComponentParticleSystem::reset_count`] the system was seeded at.
    reset_count: u32,
    /// One [`Particle`] per slot; an empty slot holds [`Particle::DEAD`].
    particles: wgpu::Buffer,
    curves: SystemCurves,
    params: wgpu::Buffer,
    steps: ResizableBuffer,
    spawns: ResizableBuffer,
    update_group: CachedBindGroup<(wgpu::BindGroupLayout, wgpu::Buffer, wgpu::Buffer)>,
    scatter_group: CachedBindGroup<(
        wgpu::BindGroupLayout,
        wgpu::Buffer,
        wgpu::Buffer,
        wgpu::Buffer,
    )>,
}

impl SimulatedSystem {
    fn new(
        device: &wgpu::Device,
        emitter: EmitterState,
        component: &ComponentParticleSystem,
    ) -> Self {
        let particles = Self::particle_buffer(device, emitter.capacity());
        Self {
            emitter,
            reset_count: component.reset_count(),
            particles,
            curves: SystemCurves::new(device, component),
            params: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("particle_simulation_params"),
                size: size_of::<SimulationParams>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            steps: ResizableBuffer::new(wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST),
            spawns: ResizableBuffer::new(
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            ),
            update_group: Default::default(),
            scatter_group: Default::default(),
        }
    }

    /// A buffer of `capacity` empty slots, at least one so it can be bound.
    fn particle_buffer(device: &wgpu::Device, capacity: u32) -> wgpu::Buffer {
        let slots = vec![Particle::DEAD; capacity.max(1) as usize];
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("particle_slots"),
            contents: bytemuck::cast_slice(&slots),
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
        })
    }

    /// Resizes the slots to `capacity`, keeping the particles of the slots
    /// both sizes have.
    fn set_capacity(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        capacity: u32,
    ) {
        let kept = self.emitter.capacity().min(capacity);
        let particles = Self::particle_buffer(device, capacity);
        if kept > 0 {
            encoder.copy_buffer_to_buffer(
                &self.particles,
                0,
                &particles,
                0,
                (kept as usize * size_of::<Particle>()) as u64,
            );
        }
        self.particles = particles;
        self.emitter.set_capacity(capacity);
    }

    /// The buffer of this system's particle slots.
    pub(crate) fn particles(&self) -> &wgpu::Buffer {
        &self.particles
    }

    /// The buffer of this system's curve keys, as `curve.wgsl` reads them.
    pub(crate) fn curve_keys(&self) -> &wgpu::Buffer {
        &self.curves.keys
    }

    /// The ranges of the size curve, then of the red, green, blue and alpha
    /// curves, in [`Self::curve_keys`].
    pub(crate) fn curve_ranges(&self) -> [GpuCurveRange; 5] {
        self.curves.ranges
    }

    /// Times the system's curve keys were uploaded.
    #[cfg(test)]
    pub(crate) fn curve_uploads(&self) -> u32 {
        self.curves.uploads
    }

    /// Number of slots.
    pub(crate) fn capacity(&self) -> u32 {
        self.emitter.capacity()
    }

    /// Number of live particles.
    pub(crate) fn live_count(&self) -> u32 {
        self.emitter.live_count()
    }

    #[cfg(test)]
    pub(crate) fn emitter(&self) -> &EmitterState {
        &self.emitter
    }

    /// Uploads the frame's steps and spawns and records the update and
    /// scatter dispatches.
    fn record(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        kernels: &SimulationKernels,
        pass: &mut wgpu::ComputePass,
    ) {
        let steps = self.emitter.steps();
        let spawns = self.emitter.spawns();
        if steps.is_empty() && spawns.is_empty() {
            return;
        }
        queue.write_buffer(
            &self.params,
            0,
            bytemuck::bytes_of(&SimulationParams {
                slot_count: self.emitter.capacity(),
                step_count: steps.len() as u32,
                spawn_count: spawns.len() as u32,
                _padding: 0,
            }),
        );
        // Bound buffers must not be empty, so each holds at least one entry.
        self.steps.resize(device, size_of::<f32>() as u64);
        self.steps.write_buffer(device, queue, steps, None);
        self.spawns
            .resize(device, size_of::<ParticleSpawn>() as u64);
        self.spawns.write_buffer(device, queue, spawns, None);

        if !steps.is_empty() {
            let layout = &kernels.update.bind_group_layouts[0];
            let steps = self.steps.get_wgpu_buffer();
            let group = self.update_group.get_or_create(
                (layout.clone(), steps.clone(), self.particles.clone()),
                || {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("particle_update_bind_group"),
                        layout,
                        entries: &[
                            buffer_entry(0, &self.params),
                            buffer_entry(1, steps),
                            buffer_entry(3, &self.particles),
                        ],
                    })
                },
            );
            let (x, y) = workgroups(self.emitter.capacity());
            pass.set_pipeline(&kernels.update.pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.dispatch_workgroups(x, y, 1);
        }
        if !spawns.is_empty() {
            let layout = &kernels.scatter.bind_group_layouts[0];
            let steps = self.steps.get_wgpu_buffer();
            let spawn_buffer = self.spawns.get_wgpu_buffer();
            let group = self.scatter_group.get_or_create(
                (
                    layout.clone(),
                    steps.clone(),
                    spawn_buffer.clone(),
                    self.particles.clone(),
                ),
                || {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("particle_scatter_bind_group"),
                        layout,
                        entries: &[
                            buffer_entry(0, &self.params),
                            buffer_entry(1, steps),
                            buffer_entry(2, spawn_buffer),
                            buffer_entry(3, &self.particles),
                        ],
                    })
                },
            );
            let (x, y) = workgroups(spawns.len() as u32);
            pass.set_pipeline(&kernels.scatter.pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.dispatch_workgroups(x, y, 1);
        }
    }
}

pub(crate) fn buffer_entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

/// Simulates the particle systems of one scene on the GPU, once per frame,
/// for every view that draws the scene.
///
/// It binds to the first scene it simulates. Given another scene, by
/// [`SceneId`], it releases all its GPU state and binds to that one, so a
/// play copy, a stopped and restarted simulation or a newly loaded scene each
/// starts from fresh systems. Within the bound scene each system is keyed by
/// its game object.
///
/// Each system's state is created the first time the simulation sees its
/// [`ComponentParticleSystem`], seeded from its object's id, and dropped on
/// the first frame its object no longer has one.
#[derive(Default)]
pub struct ParticleSimulation {
    scene: Option<SceneId>,
    shader: Option<Ref<Shader>>,
    shader_error_logged: bool,
    systems: HashMap<GameObject, SimulatedSystem>,
    frames: u64,
}

impl ParticleSimulation {
    /// Creates a simulation bound to no scene.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the scene the simulation is bound to, if any.
    pub fn scene_id(&self) -> Option<SceneId> {
        self.scene
    }

    /// Returns how many frames the simulation stepped since it last bound to
    /// a scene.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Returns the number of live particles of `game_object`'s system, or
    /// `None` when the simulation does not run one for it.
    pub fn live_count(&self, game_object: GameObject) -> Option<u32> {
        self.systems
            .get(&game_object)
            .map(SimulatedSystem::live_count)
    }

    /// Returns the game objects whose systems the simulation runs.
    pub fn systems(&self) -> impl Iterator<Item = GameObject> + '_ {
        self.systems.keys().copied()
    }

    pub(crate) fn system(&self, game_object: GameObject) -> Option<&SimulatedSystem> {
        self.systems.get(&game_object)
    }

    /// Advances every particle system of `scene` by `delta_time`.
    pub fn simulate(&mut self, scene: &Scene, render_context: &RenderContext, delta_time: f32) {
        self.simulate_steps(scene, render_context, &[delta_time]);
    }

    /// Advances every particle system of `scene` through `steps`, in order,
    /// as one frame: the CPU runs emission per step, and the GPU advances
    /// each slot through every step in one dispatch.
    ///
    /// Submits its own command buffer, so views that draw afterwards read the
    /// advanced particles.
    pub fn simulate_steps(&mut self, scene: &Scene, render_context: &RenderContext, steps: &[f32]) {
        self.bind(scene.id());
        self.frames += 1;
        let Some(kernels) = self.kernels(scene) else {
            return;
        };
        let device = render_context.device();
        let queue = render_context.queue();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("particle_simulation"),
        });
        let mut seen = HashSet::new();
        let mut query = <(Entity, &ComponentParticleSystem)>::query();
        for (entity, component) in query.iter(scene.world()) {
            let Some(game_object) = scene.game_object_from_entity(*entity) else {
                continue;
            };
            seen.insert(game_object);
            let reseed = self
                .systems
                .get(&game_object)
                .is_none_or(|system| system.reset_count != component.reset_count());
            if reseed {
                let emitter =
                    EmitterState::new(scene.uuid(game_object), component.emission.max_particles);
                self.systems.insert(
                    game_object,
                    SimulatedSystem::new(device, emitter, component),
                );
            }
            let system = self
                .systems
                .get_mut(&game_object)
                .expect("the system was just inserted");
            if system.capacity() != component.emission.max_particles {
                system.set_capacity(device, &mut encoder, component.emission.max_particles);
            }
            system.curves.update(device, queue, component);
            let emitter_transform = scene.world_transform(game_object);
            for delta_time in steps {
                system
                    .emitter
                    .step(component, *delta_time, &emitter_transform);
            }
        }
        self.systems
            .retain(|game_object, _| seen.contains(game_object));

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("particle_simulation"),
                timestamp_writes: None,
            });
            for system in self.systems.values_mut() {
                system.record(device, queue, &kernels, &mut pass);
                system.emitter.finish_frame();
            }
        }
        queue.submit(Some(encoder.finish()));
    }

    /// Releases every system when `scene` is not the bound scene, and binds
    /// to it.
    fn bind(&mut self, scene: SceneId) {
        if self.scene != Some(scene) {
            self.systems.clear();
            self.frames = 0;
            self.scene = Some(scene);
        }
    }

    fn kernels(&mut self, scene: &Scene) -> Option<SimulationKernels> {
        if self.shader.is_none() {
            match scene
                .registries()
                .assets
                .read()
                .load::<Shader>(PARTICLE_KERNELS)
            {
                Ok(shader) => self.shader = Some(shader),
                Err(error) => {
                    if !self.shader_error_logged {
                        log::error!("Particles cannot simulate: {error}");
                        self.shader_error_logged = true;
                    }
                    return None;
                }
            }
        }
        let shader = self.shader.as_ref()?.read();
        Some(SimulationKernels {
            update: shader.compute_pipeline("update_main")?,
            scatter: shader.compute_pipeline("scatter_main")?,
        })
    }

    /// Reads `game_object`'s particle slots back from the GPU.
    #[cfg(test)]
    pub(crate) fn read_particles(
        &self,
        render_context: &RenderContext,
        game_object: GameObject,
    ) -> Vec<Particle> {
        let system = &self.systems[&game_object];
        let mut particles: Vec<Particle> =
            crate::render::read_buffer(render_context, system.particles());
        particles.truncate(system.capacity() as usize);
        particles
    }
}

/// Copies `buffer` back from the GPU and waits for it.
#[cfg(test)]
pub(crate) fn read_buffer<T: bytemuck::Pod>(
    render_context: &RenderContext,
    buffer: &wgpu::Buffer,
) -> Vec<T> {
    let device = render_context.device();
    let queue = render_context.queue();
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("particle_readback"),
        size: buffer.size(),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, buffer.size());
    queue.submit(Some(encoder.finish()));
    let slice = staging.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    let _ = device.poll(wgpu::Maintain::Wait);
    receiver
        .recv()
        .expect("the map callback runs")
        .expect("the readback maps");
    let values = bytemuck::cast_slice(&slice.get_mapped_range()).to_vec();
    staging.unmap();
    values
}
