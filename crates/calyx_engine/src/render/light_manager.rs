use crate::component::{
    ComponentAmbientLight, ComponentDirectionalLight, ComponentPointLight, ComponentSpotLight,
};
use crate::render::buffer::ResizableBuffer;
use crate::render::{RenderContext, Shader};
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
    /// The light's shadow map, or -1 for none. Reserved for shadow mapping.
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
/// kind, so the shader runs one branch-free loop per kind.
#[derive(Debug, Default, PartialEq)]
struct PackedLights {
    header: LightStorageHeader,
    lights: Vec<GpuLight>,
}

impl PackedLights {
    fn new(mut lights: Vec<Light>) -> Self {
        // A stable sort keeps the scene's order within each kind.
        lights.sort_by_key(Light::upload_order);
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
            lights: lights.into_iter().map(GpuLight::from).collect(),
        }
    }
}

pub struct LightManager {
    light_storage_buffer: ResizableBuffer,
    ambient_light: [f32; 4],
}

impl Default for LightManager {
    fn default() -> Self {
        Self {
            light_storage_buffer: ResizableBuffer::new(
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            ),
            ambient_light: [0.0, 0.0, 0.0, 1.0],
        }
    }
}

impl LightManager {
    pub fn build_data(&mut self, render_context: &RenderContext, scene: &Scene) {
        let device = render_context.device();
        let queue = render_context.queue();

        let packed = PackedLights::new(Self::collect_lights(scene));
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
    }

    pub fn ambient_light(&self) -> [f32; 4] {
        self.ambient_light
    }

    /// The light storage buffer the storage bind group binds.
    pub fn storage_buffer(&self) -> wgpu::Buffer {
        self.light_storage_buffer.get_wgpu_buffer().clone()
    }

    pub fn storage_bind_group(
        &self,
        device: &wgpu::Device,
        scene_shader: &Shader,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("light_storage_bind_group"),
            layout: &scene_shader.bind_group_layouts[2],
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self
                    .light_storage_buffer
                    .get_wgpu_buffer()
                    .as_entire_binding(),
            }],
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
mod tests {
    use super::*;
    use crate::component::{
        ComponentAmbientLight, ComponentDirectionalLight, ComponentPointLight, ComponentSpotLight,
    };
    use crate::test_support::test_scene;
    use egui::Color32;
    use nalgebra_glm::{rotation, translation, vec3};

    /// The lights as the GPU reads them.
    fn gpu(lights: Vec<Light>) -> Vec<GpuLight> {
        lights.into_iter().map(GpuLight::from).collect()
    }

    #[test]
    fn collect_point_lights_skips_disabled_lights_until_re_enabled() {
        let mut scene = test_scene();
        let enabled = scene.create(None, None);
        scene.set_world_transform(enabled, translation(&vec3(1.0, 2.0, 3.0)));
        scene.add_component(
            enabled,
            ComponentPointLight {
                radius: 7.5,
                color: Color32::RED,
                intensity: 0.25,
            },
        );

        let disabled = scene.create(None, None);
        scene.add_component(
            disabled,
            ComponentPointLight {
                radius: 99.0,
                color: Color32::BLUE,
                ..Default::default()
            },
        );
        assert!(scene.set_component_enabled::<ComponentPointLight>(disabled, false));

        let lights = gpu(LightManager::collect_point_lights(&scene));

        assert_eq!(lights.len(), 1);
        assert_eq!(lights[0].position, [1.0, 2.0, 3.0]);
        assert_eq!(lights[0].range, 7.5);
        assert_eq!(lights[0].color, [1.0, 0.0, 0.0]);
        assert_eq!(lights[0].intensity, 0.25);

        assert!(scene.set_component_enabled::<ComponentPointLight>(disabled, true));
        let lights = gpu(LightManager::collect_point_lights(&scene));
        assert_eq!(lights.len(), 2);
        assert!(lights.iter().any(|light| light.range == 99.0));
    }

    #[test]
    fn collect_directional_lights_skips_disabled_lights_until_re_enabled() {
        let mut scene = test_scene();
        let enabled = scene.create(None, None);
        scene.add_component(
            enabled,
            ComponentDirectionalLight {
                color: Color32::GREEN,
                intensity: 0.5,
            },
        );

        let disabled = scene.create(None, None);
        scene.add_component(
            disabled,
            ComponentDirectionalLight {
                color: Color32::BLUE,
                ..Default::default()
            },
        );
        assert!(scene.set_component_enabled::<ComponentDirectionalLight>(disabled, false));

        let lights = gpu(LightManager::collect_directional_lights(&scene));

        assert_eq!(lights.len(), 1);
        assert_eq!(lights[0].direction, [0.0, 0.0, 1.0]);
        assert_eq!(lights[0].color, [0.0, 1.0, 0.0]);
        assert_eq!(lights[0].intensity, 0.5);

        assert!(scene.set_component_enabled::<ComponentDirectionalLight>(disabled, true));
        assert_eq!(LightManager::collect_directional_lights(&scene).len(), 2);
    }

    #[test]
    fn gpu_light_matches_the_shader_layout() {
        assert_eq!(size_of::<GpuLight>(), 64);
        assert_eq!(std::mem::offset_of!(GpuLight, range), 12);
        assert_eq!(std::mem::offset_of!(GpuLight, direction), 16);
        assert_eq!(std::mem::offset_of!(GpuLight, cos_inner), 28);
        assert_eq!(std::mem::offset_of!(GpuLight, color), 32);
        assert_eq!(std::mem::offset_of!(GpuLight, intensity), 44);
        assert_eq!(std::mem::offset_of!(GpuLight, cos_outer), 48);
        assert_eq!(std::mem::offset_of!(GpuLight, shadow_index), 52);
        // The light array starts at its 16-byte alignment, after the counts.
        assert_eq!(size_of::<LightStorageHeader>(), 16);
    }

    #[test]
    fn a_light_zeroes_the_fields_its_kind_does_not_use_and_has_no_shadow() {
        let directional = GpuLight::from(Light::Directional {
            direction: [0.0, -1.0, 0.0],
            color: [1.0, 0.5, 0.25],
            intensity: 2.0,
        });
        assert_eq!(
            directional,
            GpuLight {
                direction: [0.0, -1.0, 0.0],
                color: [1.0, 0.5, 0.25],
                intensity: 2.0,
                shadow_index: -1,
                ..Default::default()
            }
        );

        let point = GpuLight::from(Light::Point {
            position: [1.0, 2.0, 3.0],
            range: 4.0,
            color: [1.0, 1.0, 1.0],
            intensity: 0.5,
        });
        assert_eq!(
            point,
            GpuLight {
                position: [1.0, 2.0, 3.0],
                range: 4.0,
                color: [1.0, 1.0, 1.0],
                intensity: 0.5,
                shadow_index: -1,
                ..Default::default()
            }
        );
    }

    #[test]
    fn a_mixed_set_of_lights_uploads_grouped_by_kind_with_their_counts() {
        let mut scene = test_scene();
        // Created interleaved, and each identified by its intensity.
        let add_spot = |scene: &mut Scene, intensity: f32| {
            let object = scene.create(None, None);
            scene.add_component(
                object,
                ComponentSpotLight {
                    intensity,
                    ..Default::default()
                },
            );
        };
        let add_point = |scene: &mut Scene, intensity: f32| {
            let object = scene.create(None, None);
            scene.add_component(
                object,
                ComponentPointLight {
                    intensity,
                    ..Default::default()
                },
            );
        };
        let add_directional = |scene: &mut Scene, intensity: f32| {
            let object = scene.create(None, None);
            scene.add_component(
                object,
                ComponentDirectionalLight {
                    intensity,
                    ..Default::default()
                },
            );
        };
        add_spot(&mut scene, 1.0);
        add_point(&mut scene, 2.0);
        add_directional(&mut scene, 3.0);
        add_spot(&mut scene, 4.0);
        add_point(&mut scene, 5.0);
        add_spot(&mut scene, 6.0);

        let packed = PackedLights::new(LightManager::collect_lights(&scene));

        assert_eq!(
            packed.header,
            LightStorageHeader {
                directional_count: 1,
                point_count: 2,
                spot_count: 3,
                _padding: 0,
            }
        );
        assert_eq!(packed.lights.len(), 6);
        let intensities = |range: std::ops::Range<usize>| {
            let mut intensities: Vec<f32> = packed.lights[range]
                .iter()
                .map(|light| light.intensity)
                .collect();
            intensities.sort_by(f32::total_cmp);
            intensities
        };
        assert_eq!(intensities(0..1), [3.0], "directional lights come first");
        assert_eq!(intensities(1..3), [2.0, 5.0], "then the point lights");
        assert_eq!(intensities(3..6), [1.0, 4.0, 6.0], "then the spot lights");
        // Only a spot light carries cone cosines.
        assert!(packed.lights[..3]
            .iter()
            .all(|light| light.cos_outer == 0.0));
        assert!(packed.lights[3..].iter().all(|light| light.cos_outer > 0.0));
        assert!(packed.lights.iter().all(|light| light.shadow_index == -1));
    }

    #[test]
    fn no_lights_pack_to_zero_counts() {
        assert_eq!(PackedLights::new(Vec::new()), PackedLights::default());
    }

    #[test]
    fn collect_spot_lights_uses_world_position_direction_and_cone_cosines() {
        let mut scene = test_scene();
        let enabled = scene.create(None, None);
        // A quarter turn about Y takes +Z to +X.
        scene.set_world_transform(
            enabled,
            translation(&vec3(1.0, 2.0, 3.0)) * rotation(90f32.to_radians(), &vec3(0.0, 1.0, 0.0)),
        );
        scene.add_component(
            enabled,
            ComponentSpotLight {
                color: Color32::RED,
                intensity: 2.0,
                range: 6.0,
                inner_angle: 60.0,
                outer_angle: 80.0,
            },
        );

        let disabled = scene.create(None, None);
        scene.add_component(
            disabled,
            ComponentSpotLight {
                range: 99.0,
                ..Default::default()
            },
        );
        assert!(scene.set_component_enabled::<ComponentSpotLight>(disabled, false));

        let lights = gpu(LightManager::collect_spot_lights(&scene));

        assert_eq!(lights.len(), 1);
        let light = lights[0];
        assert_eq!(light.position, [1.0, 2.0, 3.0]);
        let direction = Vec3::from(light.direction);
        assert!(
            (direction - vec3(1.0, 0.0, 0.0)).norm() < 1e-5,
            "{direction}"
        );
        assert_eq!(light.range, 6.0);
        assert_eq!(light.color, [1.0, 0.0, 0.0]);
        assert_eq!(light.intensity, 2.0);
        assert!((light.cos_inner - 0.5).abs() < 1e-5, "{}", light.cos_inner);
        assert!((light.cos_outer - 80f32.to_radians().cos()).abs() < 1e-6);

        assert!(scene.set_component_enabled::<ComponentSpotLight>(disabled, true));
        let lights = gpu(LightManager::collect_spot_lights(&scene));
        assert_eq!(lights.len(), 2);
        let default = lights.iter().find(|light| light.range == 99.0).unwrap();
        assert_eq!(default.direction, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn collect_spot_lights_keeps_the_inner_cone_within_the_outer_one() {
        let mut scene = test_scene();
        let object = scene.create(None, None);
        scene.add_component(
            object,
            ComponentSpotLight {
                inner_angle: 45.0,
                outer_angle: 30.0,
                ..Default::default()
            },
        );

        let light = gpu(LightManager::collect_spot_lights(&scene))[0];

        assert_eq!(light.cos_inner, light.cos_outer);
        assert!((light.cos_outer - 30f32.to_radians().cos()).abs() < 1e-6);
    }

    #[test]
    fn collect_ambient_light_sums_enabled_lights_until_re_enabled() {
        let mut scene = test_scene();
        let first = scene.create(None, None);
        scene.add_component(
            first,
            ComponentAmbientLight {
                color: Color32::RED,
                intensity: 0.25,
            },
        );
        let second = scene.create(None, None);
        scene.add_component(
            second,
            ComponentAmbientLight {
                color: Color32::BLUE,
                intensity: 0.5,
            },
        );
        let disabled = scene.create(None, None);
        scene.add_component(
            disabled,
            ComponentAmbientLight {
                color: Color32::GREEN,
                intensity: 1.0,
            },
        );
        assert!(scene.set_component_enabled::<ComponentAmbientLight>(disabled, false));

        assert_eq!(
            LightManager::collect_ambient_light(&scene),
            [0.25, 0.0, 0.5, 1.0]
        );

        assert!(scene.set_component_enabled::<ComponentAmbientLight>(disabled, true));
        assert_eq!(
            LightManager::collect_ambient_light(&scene),
            [0.25, 1.0, 0.5, 1.0]
        );
    }
}
