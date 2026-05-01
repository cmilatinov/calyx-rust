use super::{
    Component, ComponentEventContext, ComponentReset, ComponentUpdate, ReflectComponent,
    ReflectComponentReset, ReflectComponentUpdate,
};
use crate as engine;
use crate::assets::texture::Texture;
use crate::assets::AssetRef;
use crate::input::Input;
use crate::math::Transform;
use crate::reflect::{Reflect, ReflectDefault};
use crate::render::Gizmos;
use crate::resource::ResourceMap;
use crate::scene::{GameObject, Scene};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use egui::Color32;
use nalgebra_glm::{vec3, Vec3, Vec4};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use uuid::Uuid;

const MIN_PARTICLE_LIFETIME: f32 = 0.01;
const DEFAULT_RNG_SEED: u64 = 0x9e37_79b9_7f4a_7c15;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "37c707d9-f6e8-4dc1-b2b1-159ceff99082"]
#[repr(C)]
pub enum ParticleSpawnShape {
    Sphere { radius: f32 },
    Box { extents: Vec3 },
}

impl Default for ParticleSpawnShape {
    fn default() -> Self {
        Self::Sphere { radius: 0.5 }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "6c5546bb-496e-4b68-8c1f-1d288b7665f5"]
#[serde(default)]
#[repr(C)]
pub struct ParticleScalarRange {
    pub min: f32,
    pub max: f32,
}

impl Default for ParticleScalarRange {
    fn default() -> Self {
        Self { min: 1.0, max: 1.0 }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "5af8ac8a-9652-477b-9487-68a077b3a6b3"]
#[serde(default)]
#[repr(C)]
pub struct ParticleSizeCurve {
    pub start: f32,
    pub end: f32,
    pub randomness: f32,
}

impl Default for ParticleSizeCurve {
    fn default() -> Self {
        Self {
            start: 0.25,
            end: 0.0,
            randomness: 0.05,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Particle {
    pub position_size: [f32; 4],
    pub color: [f32; 4],
    simulation_position_age: [f32; 4],
    velocity_lifetime: [f32; 4],
    acceleration_size_randomness: [f32; 4],
}

impl Particle {
    fn simulation_position(&self) -> Vec3 {
        vec3(
            self.simulation_position_age[0],
            self.simulation_position_age[1],
            self.simulation_position_age[2],
        )
    }

    fn set_simulation_position(&mut self, position: Vec3) {
        self.simulation_position_age[0] = position.x;
        self.simulation_position_age[1] = position.y;
        self.simulation_position_age[2] = position.z;
    }

    fn velocity(&self) -> Vec3 {
        vec3(
            self.velocity_lifetime[0],
            self.velocity_lifetime[1],
            self.velocity_lifetime[2],
        )
    }

    fn set_velocity(&mut self, velocity: Vec3) {
        self.velocity_lifetime[0] = velocity.x;
        self.velocity_lifetime[1] = velocity.y;
        self.velocity_lifetime[2] = velocity.z;
    }

    fn acceleration(&self) -> Vec3 {
        vec3(
            self.acceleration_size_randomness[0],
            self.acceleration_size_randomness[1],
            self.acceleration_size_randomness[2],
        )
    }

    fn set_acceleration(&mut self, acceleration: Vec3) {
        self.acceleration_size_randomness[0] = acceleration.x;
        self.acceleration_size_randomness[1] = acceleration.y;
        self.acceleration_size_randomness[2] = acceleration.z;
    }

    fn age(&self) -> f32 {
        self.simulation_position_age[3]
    }

    fn set_age(&mut self, age: f32) {
        self.simulation_position_age[3] = age;
    }

    fn lifetime(&self) -> f32 {
        self.velocity_lifetime[3]
    }

    fn set_lifetime(&mut self, lifetime: f32) {
        self.velocity_lifetime[3] = lifetime;
    }

    fn size_randomness(&self) -> f32 {
        self.acceleration_size_randomness[3]
    }

    fn set_size_randomness(&mut self, size_randomness: f32) {
        self.acceleration_size_randomness[3] = size_randomness;
    }

    fn render_position(&self) -> Vec3 {
        vec3(
            self.position_size[0],
            self.position_size[1],
            self.position_size[2],
        )
    }

    fn set_render_data(&mut self, position: Vec3, size: f32, color: [f32; 4]) {
        self.position_size = [position.x, position.y, position.z, size];
        self.color = color;
    }

    fn render_size(&self) -> f32 {
        self.position_size[3]
    }
}

#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "5214cd04-62ac-48e0-8f0b-4030d2102931"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate, ComponentReset)]
#[reflect_attr(name = "Particle System")]
#[serde(default)]
#[repr(C)]
pub struct ComponentParticleSystem {
    pub active: bool,
    pub looping: bool,
    pub local_space: bool,
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub spawn_rate: f32,
    pub burst_count: u32,
    pub max_particles: u32,
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub emission_duration: f32,
    pub lifetime: ParticleScalarRange,
    pub spawn_shape: ParticleSpawnShape,
    pub initial_velocity: Vec3,
    pub velocity_randomness: Vec3,
    pub acceleration: Vec3,
    pub size: ParticleSizeCurve,
    pub start_color: Color32,
    pub end_color: Color32,
    pub texture: AssetRef<Texture>,
    #[serde(skip)]
    #[reflect_skip]
    particles: Vec<Particle>,
    #[serde(skip)]
    #[reflect_skip]
    spawn_accumulator: f32,
    #[serde(skip)]
    #[reflect_skip]
    elapsed_time: f32,
    #[serde(skip)]
    #[reflect_skip]
    burst_emitted: bool,
    #[serde(skip)]
    #[reflect_skip]
    rng_state: u64,
}

impl Default for ComponentParticleSystem {
    fn default() -> Self {
        Self {
            active: true,
            looping: true,
            local_space: false,
            spawn_rate: 16.0,
            burst_count: 0,
            max_particles: 256,
            emission_duration: 0.0,
            lifetime: ParticleScalarRange {
                min: 0.35,
                max: 0.9,
            },
            spawn_shape: Default::default(),
            initial_velocity: vec3(0.0, 1.0, 0.0),
            velocity_randomness: vec3(0.5, 0.5, 0.5),
            acceleration: vec3(0.0, -1.5, 0.0),
            size: Default::default(),
            start_color: Color32::WHITE,
            end_color: Color32::from_rgba_unmultiplied(255, 255, 255, 0),
            texture: Default::default(),
            particles: Vec::new(),
            spawn_accumulator: 0.0,
            elapsed_time: 0.0,
            burst_emitted: false,
            rng_state: DEFAULT_RNG_SEED,
        }
    }
}

impl Component for ComponentParticleSystem {
    fn draw_gizmos(&self, scene: &Scene, game_object: GameObject, gizmos: &mut Gizmos) {
        let transform = scene.world_transform(game_object);
        gizmos.set_color(&Vec4::new(1.0, 0.6, 0.2, 1.0));
        match self.spawn_shape {
            ParticleSpawnShape::Sphere { radius } => {
                gizmos.wire_sphere(&transform.position, radius.max(0.0));
            }
            ParticleSpawnShape::Box { extents } => {
                gizmos.wire_cube(&transform.position, &(extents * 2.0));
            }
        }
    }
}

impl ComponentReset for ComponentParticleSystem {
    fn reset(
        &self,
        ComponentEventContext {
            scene, game_object, ..
        }: ComponentEventContext,
    ) {
        let seed = scene.uuid(game_object);
        scene.write_component::<ComponentParticleSystem, _>(game_object, |system| {
            system.reset_runtime(seed);
        });
    }
}

impl ComponentUpdate for ComponentParticleSystem {
    fn update(
        &self,
        ComponentEventContext {
            scene, game_object, ..
        }: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        let emitter_transform = scene.world_transform(game_object);
        let delta_time = resources.time().delta_time();
        scene.write_component::<ComponentParticleSystem, _>(game_object, |system| {
            system.step(delta_time, &emitter_transform);
        });
    }
}

impl ComponentParticleSystem {
    pub(crate) fn prepare_render_data(
        &mut self,
        emitter_transform: &Transform,
        camera_position: &Vec3,
    ) -> usize {
        let size_start = self.size.start;
        let size_end = self.size.end;
        let start_color = self.start_color;
        let end_color = self.end_color;
        let local_space = self.local_space;
        for particle in &mut self.particles {
            let lifetime = particle.lifetime().max(MIN_PARTICLE_LIFETIME);
            let t = (particle.age() / lifetime).clamp(0.0, 1.0);
            let size_randomness = particle.size_randomness();
            let start = (size_start + size_randomness).max(0.0);
            let end = (size_end + size_randomness).max(0.0);
            let size = start + (end - start) * t;
            let simulation_position = particle.simulation_position();
            let world_position = if local_space {
                emitter_transform.transform_position(&simulation_position)
            } else {
                simulation_position
            };
            particle.set_render_data(
                world_position,
                size.max(0.0),
                Self::lerp_color(start_color, end_color, t),
            );
        }

        self.particles.sort_by(|left, right| {
            let left_visible = left.render_size() > 0.0;
            let right_visible = right.render_size() > 0.0;
            right_visible.cmp(&left_visible).then_with(|| {
                let right_offset = right.render_position() - *camera_position;
                let left_offset = left.render_position() - *camera_position;
                right_offset
                    .dot(&right_offset)
                    .partial_cmp(&left_offset.dot(&left_offset))
                    .unwrap_or(Ordering::Equal)
            })
        });
        self.particles
            .iter()
            .take_while(|particle| particle.render_size() > 0.0)
            .count()
    }

    pub(crate) fn render_particles(&self, count: usize) -> &[Particle] {
        &self.particles[..count]
    }

    pub(crate) fn step(&mut self, delta_time: f32, emitter_transform: &Transform) {
        if delta_time <= 0.0 {
            if self.active && !self.burst_emitted {
                self.emit_burst(emitter_transform);
            }
            return;
        }

        self.update_particles(delta_time);

        if !self.active {
            return;
        }

        let emission_delta_time = self.advance_emission_time(delta_time);
        if !self.burst_emitted {
            self.emit_burst(emitter_transform);
        }

        if emission_delta_time > 0.0 && self.spawn_rate > 0.0 {
            self.spawn_accumulator += emission_delta_time * self.spawn_rate.max(0.0);
            let particles_to_spawn = self.spawn_accumulator.floor() as u32;
            if particles_to_spawn > 0 {
                self.spawn_accumulator -= particles_to_spawn as f32;
                self.spawn_particles(particles_to_spawn, emitter_transform);
            }
        }
    }

    fn advance_emission_time(&mut self, delta_time: f32) -> f32 {
        if self.emission_duration <= 0.0 {
            self.elapsed_time += delta_time;
            return delta_time;
        }

        let previous_elapsed = self.elapsed_time;
        self.elapsed_time += delta_time;

        if self.looping {
            while self.elapsed_time >= self.emission_duration {
                self.elapsed_time -= self.emission_duration;
                self.burst_emitted = false;
            }
            delta_time
        } else {
            (self.emission_duration - previous_elapsed).clamp(0.0, delta_time)
        }
    }

    fn emit_burst(&mut self, emitter_transform: &Transform) {
        self.burst_emitted = true;
        if self.burst_count > 0 {
            self.spawn_particles(self.burst_count, emitter_transform);
        }
    }

    fn spawn_particles(&mut self, count: u32, emitter_transform: &Transform) {
        let available = self
            .max_particles
            .saturating_sub(self.particles.len() as u32) as usize;
        for _ in 0..available.min(count as usize) {
            let local_position = self.sample_spawn_position();
            let local_velocity = self.initial_velocity
                + vec3(
                    self.random_signed() * self.velocity_randomness.x,
                    self.random_signed() * self.velocity_randomness.y,
                    self.random_signed() * self.velocity_randomness.z,
                );
            let position = if self.local_space {
                local_position
            } else {
                emitter_transform.transform_position(&local_position)
            };
            let velocity = if self.local_space {
                local_velocity
            } else {
                emitter_transform.transform_direction(&local_velocity)
            };
            let acceleration = if self.local_space {
                self.acceleration
            } else {
                emitter_transform.transform_direction(&self.acceleration)
            };
            let lifetime = self.sample_range(self.lifetime).max(MIN_PARTICLE_LIFETIME);
            let size_randomness = self.random_signed() * self.size.randomness.max(0.0);

            let mut particle = Particle::default();
            particle.set_simulation_position(position);
            particle.set_velocity(velocity);
            particle.set_acceleration(acceleration);
            particle.set_age(0.0);
            particle.set_lifetime(lifetime);
            particle.set_size_randomness(size_randomness);
            self.particles.push(particle);
        }
    }

    fn update_particles(&mut self, delta_time: f32) {
        for particle in &mut self.particles {
            particle.set_age(particle.age() + delta_time);
            let velocity = particle.velocity() + particle.acceleration() * delta_time;
            let position = particle.simulation_position() + velocity * delta_time;
            particle.set_velocity(velocity);
            particle.set_simulation_position(position);
        }
        self.particles
            .retain(|particle| particle.age() < particle.lifetime().max(MIN_PARTICLE_LIFETIME));
    }

    fn sample_spawn_position(&mut self) -> Vec3 {
        match self.spawn_shape {
            ParticleSpawnShape::Sphere { radius } => self.random_in_unit_sphere() * radius,
            ParticleSpawnShape::Box { extents } => vec3(
                self.random_signed() * extents.x,
                self.random_signed() * extents.y,
                self.random_signed() * extents.z,
            ),
        }
    }

    fn sample_range(&mut self, range: ParticleScalarRange) -> f32 {
        if (range.max - range.min).abs() <= f32::EPSILON {
            range.min
        } else {
            range.min + self.random_scalar() * (range.max - range.min)
        }
    }

    fn random_in_unit_sphere(&mut self) -> Vec3 {
        for _ in 0..16 {
            let point = vec3(
                self.random_signed(),
                self.random_signed(),
                self.random_signed(),
            );
            if point.dot(&point) <= 1.0 {
                return point;
            }
        }
        Vec3::zeros()
    }

    fn random_scalar(&mut self) -> f32 {
        self.rng_state ^= self.rng_state << 13;
        self.rng_state ^= self.rng_state >> 7;
        self.rng_state ^= self.rng_state << 17;
        ((self.rng_state >> 32) as u32) as f32 / (u32::MAX as f32)
    }

    fn random_signed(&mut self) -> f32 {
        self.random_scalar() * 2.0 - 1.0
    }

    pub(crate) fn reset_runtime(&mut self, seed: Uuid) {
        self.particles.clear();
        self.spawn_accumulator = 0.0;
        self.elapsed_time = 0.0;
        self.burst_emitted = false;
        self.rng_state = seed.as_u128() as u64 ^ DEFAULT_RNG_SEED;
    }

    fn lerp_color(start: Color32, end: Color32, t: f32) -> [f32; 4] {
        let start = start.to_normalized_gamma_f32();
        let end = end.to_normalized_gamma_f32();
        [
            start[0] + (end[0] - start[0]) * t,
            start[1] + (end[1] - start[1]) * t,
            start[2] + (end[2] - start[2]) * t,
            start[3] + (end[3] - start[3]) * t,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Scene;
    use crate::test_utils::test_registries;
    use nalgebra_glm::Vec3;

    fn add_particle_system(scene: &mut Scene, game_object: GameObject) {
        scene.add_component(
            game_object,
            ComponentParticleSystem {
                spawn_rate: 4.0,
                burst_count: 2,
                lifetime: ParticleScalarRange { min: 1.0, max: 1.0 },
                size: ParticleSizeCurve {
                    start: 1.0,
                    end: 0.0,
                    randomness: 0.0,
                },
                initial_velocity: Vec3::zeros(),
                velocity_randomness: Vec3::zeros(),
                acceleration: Vec3::zeros(),
                max_particles: 16,
                ..Default::default()
            },
        );
    }

    #[test]
    fn particle_system_emits_burst_and_rate_particles() {
        let registries = test_registries();
        let mut scene = registries.scene();
        let go = scene.create(None, None);
        add_particle_system(&mut scene, go);

        let emitter_transform = scene.world_transform(go);
        let seed = scene.uuid(go);
        scene.write_component::<ComponentParticleSystem, _>(go, |system| {
            system.reset_runtime(seed);
            system.step(0.5, &emitter_transform);
        });

        let particle_count = scene
            .read_component::<ComponentParticleSystem, _, _>(go, |system| system.particles.len())
            .unwrap_or_default();
        assert_eq!(particle_count, 4);
    }

    #[test]
    fn local_space_particles_follow_emitter_transform() {
        let registries = test_registries();
        let mut scene = registries.scene();
        let go = scene.create(None, None);
        scene.add_component(
            go,
            ComponentParticleSystem {
                local_space: true,
                spawn_rate: 0.0,
                burst_count: 1,
                lifetime: ParticleScalarRange { min: 1.0, max: 1.0 },
                size: ParticleSizeCurve {
                    start: 1.0,
                    end: 1.0,
                    randomness: 0.0,
                },
                initial_velocity: Vec3::zeros(),
                velocity_randomness: Vec3::zeros(),
                acceleration: Vec3::zeros(),
                spawn_shape: ParticleSpawnShape::Sphere { radius: 0.0 },
                ..Default::default()
            },
        );

        let emitter_transform = scene.world_transform(go);
        let seed = scene.uuid(go);
        scene.write_component::<ComponentParticleSystem, _>(go, |system| {
            system.reset_runtime(seed);
            system.step(0.0, &emitter_transform);
        });
        scene.set_world_transform(go, Transform::from_xyz(5.0, 0.0, 0.0).matrix());

        let expected_x = scene.world_transform(go).position.x;
        let transform = scene.world_transform(go);
        scene.write_component::<ComponentParticleSystem, _>(go, |system| {
            system.prepare_render_data(&transform, &Vec3::zeros());
        });
        let moved_position = scene
            .read_component::<ComponentParticleSystem, _, _>(go, |system| {
                system.render_particles(1)[0].position_size
            })
            .unwrap();

        assert!((moved_position[0] - expected_x).abs() < 1e-5);
    }

    #[test]
    fn particle_velocity_randomness_varies_per_particle() {
        let registries = test_registries();
        let mut scene = registries.scene();
        let go = scene.create(None, None);
        scene.add_component(
            go,
            ComponentParticleSystem {
                local_space: true,
                spawn_rate: 0.0,
                burst_count: 4,
                lifetime: ParticleScalarRange { min: 1.0, max: 1.0 },
                size: ParticleSizeCurve {
                    start: 1.0,
                    end: 1.0,
                    randomness: 0.0,
                },
                initial_velocity: Vec3::zeros(),
                velocity_randomness: Vec3::from_element(1.0),
                acceleration: Vec3::zeros(),
                ..Default::default()
            },
        );

        let emitter_transform = scene.world_transform(go);
        let seed = scene.uuid(go);
        scene.write_component::<ComponentParticleSystem, _>(go, |system| {
            system.reset_runtime(seed);
            system.step(0.0, &emitter_transform);
        });

        let distinct_velocities = scene
            .read_component::<ComponentParticleSystem, _, _>(go, |system| {
                let mut distinct = Vec::<Vec3>::new();
                for particle in &system.particles {
                    if distinct
                        .iter()
                        .all(|existing| (particle.velocity() - *existing).norm() > 1e-4)
                    {
                        distinct.push(particle.velocity());
                    }
                }
                distinct.len()
            })
            .unwrap_or_default();

        assert!(
            distinct_velocities > 1,
            "expected multiple distinct particle velocities"
        );
    }

    #[test]
    fn particle_size_changes_over_time() {
        let registries = test_registries();
        let mut scene = registries.scene();
        let go = scene.create(None, None);
        scene.add_component(
            go,
            ComponentParticleSystem {
                local_space: true,
                spawn_rate: 0.0,
                burst_count: 1,
                lifetime: ParticleScalarRange { min: 1.0, max: 1.0 },
                size: ParticleSizeCurve {
                    start: 1.0,
                    end: 0.25,
                    randomness: 0.0,
                },
                initial_velocity: Vec3::zeros(),
                velocity_randomness: Vec3::zeros(),
                acceleration: Vec3::zeros(),
                ..Default::default()
            },
        );

        let emitter_transform = scene.world_transform(go);
        let seed = scene.uuid(go);
        scene.write_component::<ComponentParticleSystem, _>(go, |system| {
            system.reset_runtime(seed);
            system.step(0.0, &emitter_transform);
        });

        scene.write_component::<ComponentParticleSystem, _>(go, |system| {
            system.prepare_render_data(&emitter_transform, &Vec3::zeros());
        });
        let initial_size = scene
            .read_component::<ComponentParticleSystem, _, _>(go, |system| {
                system.render_particles(1)[0].render_size()
            })
            .unwrap();

        scene.write_component::<ComponentParticleSystem, _>(go, |system| {
            system.step(0.5, &emitter_transform);
        });

        scene.write_component::<ComponentParticleSystem, _>(go, |system| {
            system.prepare_render_data(&emitter_transform, &Vec3::zeros());
        });
        let later_size = scene
            .read_component::<ComponentParticleSystem, _, _>(go, |system| {
                system.render_particles(1)[0].render_size()
            })
            .unwrap();

        assert!(later_size < initial_size);
        assert!((later_size - 0.625).abs() < 1e-5);
    }
}
