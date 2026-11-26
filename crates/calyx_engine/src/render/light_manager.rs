use crate::component::{
    ComponentAmbientLight, ComponentDirectionalLight, ComponentPointLight, ComponentSpotLight,
};
use crate::render::buffer::ResizableBuffer;
use crate::render::{
    RenderContext, Shader, ShadowBindings, ShadowCamera, ShadowCaster, ShadowPlan, ShadowSettings,
};
use crate::scene::Scene;
use egui_wgpu::wgpu;
use legion::{Entity, IntoQuery};
use nalgebra_glm::Vec3;
use std::mem::size_of;

/// An enabled light gathered from the scene, before it is packed for the GPU.
#[derive(Copy, Clone, Debug, PartialEq)]
enum Light {
    Directional {
        /// World direction the light travels along.
        direction: [f32; 3],
        color: [f32; 3],
        intensity: f32,
        /// Its shadow settings when it casts shadows.
        shadow: Option<ShadowSettings>,
    },
    Point {
        position: [f32; 3],
        range: f32,
        color: [f32; 3],
        intensity: f32,
    },
    Spot {
        position: [f32; 3],
        /// Unit world direction the cone opens along.
        direction: [f32; 3],
        range: f32,
        /// Cosines of the inner and outer cone half-angles, with
        /// `cos_inner >= cos_outer`, so the shader never takes a cosine per
        /// pixel.
        cos_inner: f32,
        cos_outer: f32,
        color: [f32; 3],
        intensity: f32,
        /// Its shadow settings when it casts shadows.
        shadow: Option<ShadowSettings>,
    },
}

impl Light {
    /// The kind's place in the light storage buffer: every directional
    /// light, then every point light, then every spot light.
    fn upload_order(&self) -> usize {
        match self {
            Light::Directional { .. } => 0,
            Light::Point { .. } => 1,
            Light::Spot { .. } => 2,
        }
    }

    /// The light as a shadow caster, or `None` when it casts no shadow.
    fn shadow_caster(&self) -> Option<ShadowCaster> {
        match *self {
            Light::Directional {
                direction,
                shadow: Some(settings),
                ..
            } => Some(ShadowCaster::Directional {
                direction: direction.into(),
                settings,
            }),
            Light::Spot {
                position,
                direction,
                range,
                cos_outer,
                shadow: Some(settings),
                ..
            } => Some(ShadowCaster::Spot {
                position: position.into(),
                direction: direction.into(),
                range,
                outer_angle: cos_outer.clamp(-1.0, 1.0).acos(),
                settings,
            }),
            _ => None,
        }
    }
}

/// A light as `Light` in `assets/shaders/common/lights.wgsl` reads it. Every
/// kind shares this one layout; a kind reads only its own fields, and the
/// others are zero.
#[repr(C)]
#[derive(Default, Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuLight {
    position: [f32; 3],
    range: f32,
    direction: [f32; 3],
    cos_inner: f32,
    color: [f32; 3],
    intensity: f32,
    cos_outer: f32,
    /// The light's shadow, or -1 for none: the cascades of a directional
    /// light, or the atlas tile of a spot light (see [`ShadowPlan`]).
    shadow_index: i32,
    _padding: [f32; 2],
}

impl From<Light> for GpuLight {
    fn from(light: Light) -> Self {
        let unused = GpuLight {
            shadow_index: -1,
            ..Default::default()
        };
        match light {
            Light::Directional {
                direction,
                color,
                intensity,
                ..
            } => GpuLight {
                direction,
                color,
                intensity,
                ..unused
            },
            Light::Point {
                position,
                range,
                color,
                intensity,
            } => GpuLight {
                position,
                range,
                color,
                intensity,
                ..unused
            },
            Light::Spot {
                position,
                direction,
                range,
                cos_inner,
                cos_outer,
                color,
                intensity,
                ..
            } => GpuLight {
                position,
                range,
                direction,
                cos_inner,
                color,
                intensity,
                cos_outer,
                ..unused
            },
        }
    }
}

/// The header of `LightStorage` in `assets/shaders/common/lights.wgsl`: how
/// many lights of each kind follow it, padded to the light array's 16-byte
/// alignment.
#[repr(C)]
#[derive(Default, Copy, Clone, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
struct LightStorageHeader {
    directional_count: u32,
    point_count: u32,
    spot_count: u32,
    _padding: u32,
}

/// The contents of the light storage buffer: the header, then the lights
/// grouped by kind in the header's order. The range a light sits in is its
/// kind, so the shader runs one branch-free loop per kind. Each light carries
/// its index into `shadows`.
#[derive(Debug, Default, PartialEq)]
struct PackedLights {
    header: LightStorageHeader,
    lights: Vec<GpuLight>,
    shadows: ShadowPlan,
}

impl PackedLights {
    /// Packs `lights`, giving shadows fitted to `camera` to those that cast
    /// them; without a camera no light has a shadow.
    fn new(mut lights: Vec<Light>, camera: Option<&ShadowCamera>) -> Self {
        // A stable sort keeps the scene's order within each kind, so the
        // first shadowed directional light is the scene's first.
        lights.sort_by_key(Light::upload_order);
        let casters: Vec<Option<ShadowCaster>> = lights.iter().map(Light::shadow_caster).collect();
        let (shadows, shadow_indices) = ShadowPlan::new(&casters, camera);
        let count = |order: usize| {
            lights
                .iter()
                .filter(|light| light.upload_order() == order)
                .count() as u32
        };
        Self {
            header: LightStorageHeader {
                directional_count: count(0),
                point_count: count(1),
                spot_count: count(2),
                _padding: 0,
            },
            lights: lights
                .into_iter()
                .zip(shadow_indices)
                .map(|(light, shadow_index)| GpuLight {
                    shadow_index,
                    ..GpuLight::from(light)
                })
                .collect(),
            shadows,
        }
    }
}

pub struct LightManager {
    light_storage_buffer: ResizableBuffer,
    ambient_light: [f32; 4],
    shadow_plan: ShadowPlan,
}

impl Default for LightManager {
    fn default() -> Self {
        Self {
            light_storage_buffer: ResizableBuffer::new(
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            ),
            ambient_light: [0.0, 0.0, 0.0, 1.0],
            shadow_plan: Default::default(),
        }
    }
}

impl LightManager {
    /// Gathers and uploads the scene's enabled lights. With a camera, the
    /// lights that cast shadows get them, fitted to it; see
    /// [`Self::shadow_plan`].
    pub fn build_data(
        &mut self,
        render_context: &RenderContext,
        scene: &Scene,
        shadow_camera: Option<&ShadowCamera>,
    ) {
        let device = render_context.device();
        let queue = render_context.queue();

        let packed = PackedLights::new(Self::collect_lights(scene), shadow_camera);
        let header_size = size_of::<LightStorageHeader>();
        // The buffer holds room for at least one light, even with none to light.
        let size = (header_size + packed.lights.len().max(1) * size_of::<GpuLight>()) as u64;
        self.light_storage_buffer.resize(device, size);
        self.light_storage_buffer
            .write_buffer(device, queue, &[packed.header], None);
        if !packed.lights.is_empty() {
            self.light_storage_buffer.write_buffer(
                device,
                queue,
                packed.lights.as_slice(),
                Some(header_size as u64),
            );
        }

        self.ambient_light = Self::collect_ambient_light(scene);
        self.shadow_plan = packed.shadows;
    }

    /// The shadows the last [`Self::build_data`] gave its lights.
    pub fn shadow_plan(&self) -> &ShadowPlan {
        &self.shadow_plan
    }

    pub fn ambient_light(&self) -> [f32; 4] {
        self.ambient_light
    }

    /// The light storage buffer the storage bind group binds.
    pub fn storage_buffer(&self) -> wgpu::Buffer {
        self.light_storage_buffer.get_wgpu_buffer().clone()
    }

    /// The group 2 bind group of `scene_shader`: the light storage buffer,
    /// then the shadow data, maps and sampler of
    /// `assets/shaders/common/shadows.wgsl`.
    pub fn storage_bind_group(
        &self,
        device: &wgpu::Device,
        scene_shader: &Shader,
        shadows: &ShadowBindings<'_>,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("light_storage_bind_group"),
            layout: &scene_shader.bind_group_layouts[2],
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self
                        .light_storage_buffer
                        .get_wgpu_buffer()
                        .as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: shadows.data.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(shadows.cascades),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(shadows.atlas),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(shadows.sampler),
                },
            ],
        })
    }

    /// Every enabled directional, point and spot light, in no particular
    /// order of kind; `PackedLights` groups them.
    fn collect_lights(scene: &Scene) -> Vec<Light> {
        let mut lights = Self::collect_point_lights(scene);
        lights.extend(Self::collect_directional_lights(scene));
        lights.extend(Self::collect_spot_lights(scene));
        lights
    }

    fn collect_point_lights(scene: &Scene) -> Vec<Light> {
        let mut point_lights = Vec::new();
        let mut query = <(Entity, &ComponentPointLight)>::query();
        for (game_object, light) in query
            .iter(scene.world())
            .filter_map(|(entity, light)| {
                scene.game_object_from_entity(*entity).map(|go| (go, light))
            })
            .filter(|(go, _)| scene.is_component_enabled::<ComponentPointLight>(*go))
        {
            let color = light.color.to_normalized_gamma_f32();
            point_lights.push(Light::Point {
                position: scene.world_transform(game_object).position.into(),
                range: light.radius,
                color: [color[0], color[1], color[2]],
                intensity: light.intensity.max(0.0),
            });
        }
        point_lights
    }

    fn collect_directional_lights(scene: &Scene) -> Vec<Light> {
        let mut directional_lights = Vec::new();
        let mut query = <(Entity, &ComponentDirectionalLight)>::query();
        for (game_object, light) in query
            .iter(scene.world())
            .filter_map(|(entity, light)| {
                scene.game_object_from_entity(*entity).map(|go| (go, light))
            })
            .filter(|(go, _)| scene.is_component_enabled::<ComponentDirectionalLight>(*go))
        {
            let color = light.color.to_normalized_gamma_f32();
            directional_lights.push(Light::Directional {
                direction: scene
                    .world_transform(game_object)
                    .transform_direction(&Vec3::z_axis())
                    .into(),
                color: [color[0], color[1], color[2]],
                intensity: light.intensity.max(0.0),
                shadow: light.cast_shadows.then_some(ShadowSettings {
                    distance: light.shadow_distance.max(1.0),
                    depth_bias: light.depth_bias,
                    normal_bias: light.normal_bias,
                }),
            })
        }
        directional_lights
    }

    /// Every enabled spot light, pointing along its object's world +Z like a
    /// directional light, with its cone cosines.
    fn collect_spot_lights(scene: &Scene) -> Vec<Light> {
        let mut spot_lights = Vec::new();
        let mut query = <(Entity, &ComponentSpotLight)>::query();
        for (game_object, light) in query
            .iter(scene.world())
            .filter_map(|(entity, light)| {
                scene.game_object_from_entity(*entity).map(|go| (go, light))
            })
            .filter(|(go, _)| scene.is_component_enabled::<ComponentSpotLight>(*go))
        {
            let transform = scene.world_transform(game_object);
            let direction = transform.transform_direction(&Vec3::z_axis());
            let direction = if direction.norm_squared() > f32::EPSILON {
                direction.normalize()
            } else {
                Vec3::z()
            };
            let (inner, outer) = light.cone_angles();
            let color = light.color.to_normalized_gamma_f32();
            spot_lights.push(Light::Spot {
                position: transform.position.into(),
                direction: direction.into(),
                range: light.range.max(0.0),
                cos_inner: inner.to_radians().cos(),
                cos_outer: outer.to_radians().cos(),
                color: [color[0], color[1], color[2]],
                intensity: light.intensity.max(0.0),
                shadow: light.cast_shadows.then_some(ShadowSettings {
                    distance: light.shadow_distance.max(0.0),
                    depth_bias: light.depth_bias,
                    normal_bias: light.normal_bias,
                }),
            });
        }
        spot_lights
    }

    fn collect_ambient_light(scene: &Scene) -> [f32; 4] {
        let mut ambient = [0.0, 0.0, 0.0, 1.0];
        let mut query = <(Entity, &ComponentAmbientLight)>::query();
        for (_, light) in query.iter(scene.world()).filter(|(entity, _)| {
            scene
                .game_object_from_entity(**entity)
                .is_some_and(|go| scene.is_component_enabled::<ComponentAmbientLight>(go))
        }) {
            let color = light.color.to_normalized_gamma_f32();
            let intensity = light.intensity.max(0.0);
            ambient[0] += color[0] * intensity;
            ambient[1] += color[1] * intensity;
            ambient[2] += color[2] * intensity;
        }
        ambient
    }
}

#[cfg(test)]
mod tests;
