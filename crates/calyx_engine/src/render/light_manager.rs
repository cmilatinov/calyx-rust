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

#[repr(C)]
#[derive(Default, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct PointLight {
    position: [f32; 3],
    radius: f32,
    color: [f32; 3],
    intensity: f32,
}

#[repr(C)]
#[derive(Default, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct DirectionalLight {
    direction: [f32; 3],
    _padding: f32,
    color: [f32; 3],
    intensity: f32,
}

/// A spot light as `assets/shaders/common/lights.wgsl` reads it. The cone
/// cosines are computed here, so the shader never takes a cosine per pixel.
#[repr(C)]
#[derive(Default, Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct SpotLight {
    position: [f32; 3],
    range: f32,
    /// Unit world direction the cone opens along.
    direction: [f32; 3],
    cos_inner: f32,
    color: [f32; 3],
    intensity: f32,
    cos_outer: f32,
    _padding: [f32; 3],
}

pub struct LightManager {
    point_light_storage_buffer: ResizableBuffer,
    directional_light_storage_buffer: ResizableBuffer,
    spot_light_storage_buffer: ResizableBuffer,
    ambient_light: [f32; 4],
}

impl Default for LightManager {
    fn default() -> Self {
        Self {
            point_light_storage_buffer: ResizableBuffer::new(
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            ),
            directional_light_storage_buffer: ResizableBuffer::new(
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            ),
            spot_light_storage_buffer: ResizableBuffer::new(
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

        let point_lights = Self::collect_point_lights(scene);
        let size = (16 + std::cmp::max(point_lights.len(), 1) * size_of::<PointLight>()) as u64;
        self.point_light_storage_buffer.resize(device, size);
        self.point_light_storage_buffer.write_buffer(
            device,
            queue,
            &[point_lights.len() as u32],
            None,
        );
        if !point_lights.is_empty() {
            self.point_light_storage_buffer.write_buffer(
                device,
                queue,
                point_lights.as_slice(),
                Some(16),
            );
        }

        let directional_lights = Self::collect_directional_lights(scene);
        let size = (16 + std::cmp::max(directional_lights.len(), 1) * size_of::<DirectionalLight>())
            as u64;
        self.directional_light_storage_buffer.resize(device, size);
        self.directional_light_storage_buffer.write_buffer(
            device,
            queue,
            &[directional_lights.len() as u32],
            None,
        );
        if !directional_lights.is_empty() {
            self.directional_light_storage_buffer.write_buffer(
                device,
                queue,
                directional_lights.as_slice(),
                Some(16),
            );
        }

        let spot_lights = Self::collect_spot_lights(scene);
        let size = (16 + std::cmp::max(spot_lights.len(), 1) * size_of::<SpotLight>()) as u64;
        self.spot_light_storage_buffer.resize(device, size);
        self.spot_light_storage_buffer.write_buffer(
            device,
            queue,
            &[spot_lights.len() as u32],
            None,
        );
        if !spot_lights.is_empty() {
            self.spot_light_storage_buffer.write_buffer(
                device,
                queue,
                spot_lights.as_slice(),
                Some(16),
            );
        }

        self.ambient_light = Self::collect_ambient_light(scene);
    }

    pub fn ambient_light(&self) -> [f32; 4] {
        self.ambient_light
    }

    /// The point, directional and spot light storage buffers the storage
    /// bind group binds, in binding order.
    pub fn storage_buffers(&self) -> [wgpu::Buffer; 3] {
        [
            self.point_light_storage_buffer.get_wgpu_buffer().clone(),
            self.directional_light_storage_buffer
                .get_wgpu_buffer()
                .clone(),
            self.spot_light_storage_buffer.get_wgpu_buffer().clone(),
        ]
    }

    pub fn storage_bind_group(
        &self,
        device: &wgpu::Device,
        scene_shader: &Shader,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("light_storage_bind_group"),
            layout: &scene_shader.bind_group_layouts[2],
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self
                        .point_light_storage_buffer
                        .get_wgpu_buffer()
                        .as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self
                        .directional_light_storage_buffer
                        .get_wgpu_buffer()
                        .as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self
                        .spot_light_storage_buffer
                        .get_wgpu_buffer()
                        .as_entire_binding(),
                },
            ],
        })
    }

    fn collect_point_lights(scene: &Scene) -> Vec<PointLight> {
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
            point_lights.push(PointLight {
                color: [color[0], color[1], color[2]],
                intensity: light.intensity.max(0.0),
                radius: light.radius,
                position: scene.world_transform(game_object).position.into(),
            });
        }
        point_lights
    }

    fn collect_directional_lights(scene: &Scene) -> Vec<DirectionalLight> {
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
            directional_lights.push(DirectionalLight {
                color: [color[0], color[1], color[2]],
                intensity: light.intensity.max(0.0),
                _padding: 0.0,
                direction: scene
                    .world_transform(game_object)
                    .transform_direction(&Vec3::z_axis())
                    .into(),
            })
        }
        directional_lights
    }

    /// Every enabled spot light, pointing along its object's world +Z like a
    /// directional light, with its cone cosines.
    fn collect_spot_lights(scene: &Scene) -> Vec<SpotLight> {
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
            spot_lights.push(SpotLight {
                position: transform.position.into(),
                range: light.range.max(0.0),
                direction: direction.into(),
                cos_inner: inner.to_radians().cos(),
                color: [color[0], color[1], color[2]],
                intensity: light.intensity.max(0.0),
                cos_outer: outer.to_radians().cos(),
                _padding: [0.0; 3],
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

        let lights = LightManager::collect_point_lights(&scene);

        assert_eq!(lights.len(), 1);
        assert_eq!(lights[0].position, [1.0, 2.0, 3.0]);
        assert_eq!(lights[0].radius, 7.5);
        assert_eq!(lights[0].color, [1.0, 0.0, 0.0]);
        assert_eq!(lights[0].intensity, 0.25);

        assert!(scene.set_component_enabled::<ComponentPointLight>(disabled, true));
        let lights = LightManager::collect_point_lights(&scene);
        assert_eq!(lights.len(), 2);
        assert!(lights.iter().any(|light| light.radius == 99.0));
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

        let lights = LightManager::collect_directional_lights(&scene);

        assert_eq!(lights.len(), 1);
        assert_eq!(lights[0].direction, [0.0, 0.0, 1.0]);
        assert_eq!(lights[0].color, [0.0, 1.0, 0.0]);
        assert_eq!(lights[0].intensity, 0.5);

        assert!(scene.set_component_enabled::<ComponentDirectionalLight>(disabled, true));
        assert_eq!(LightManager::collect_directional_lights(&scene).len(), 2);
    }

    #[test]
    fn spot_light_matches_the_shader_layout() {
        assert_eq!(size_of::<SpotLight>(), 64);
        assert_eq!(std::mem::offset_of!(SpotLight, range), 12);
        assert_eq!(std::mem::offset_of!(SpotLight, direction), 16);
        assert_eq!(std::mem::offset_of!(SpotLight, cos_inner), 28);
        assert_eq!(std::mem::offset_of!(SpotLight, color), 32);
        assert_eq!(std::mem::offset_of!(SpotLight, intensity), 44);
        assert_eq!(std::mem::offset_of!(SpotLight, cos_outer), 48);
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

        let lights = LightManager::collect_spot_lights(&scene);

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
        let lights = LightManager::collect_spot_lights(&scene);
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

        let light = LightManager::collect_spot_lights(&scene)[0];

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
