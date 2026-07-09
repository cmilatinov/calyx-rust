use crate::component::{ComponentAmbientLight, ComponentDirectionalLight, ComponentPointLight};
use crate::render::buffer::ResizableBuffer;
use crate::render::Shader;
use crate::scene::Scene;
use egui_wgpu::{wgpu, RenderState};
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

pub struct LightManager {
    point_light_storage_buffer: ResizableBuffer,
    directional_light_storage_buffer: ResizableBuffer,
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
            ambient_light: [0.0, 0.0, 0.0, 1.0],
        }
    }
}

impl LightManager {
    pub fn build_data(&mut self, render_state: &RenderState, scene: &Scene) {
        let device = &render_state.device;
        let queue = &render_state.queue;

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

        self.ambient_light = Self::collect_ambient_light(scene);
    }

    pub fn ambient_light(&self) -> [f32; 4] {
        self.ambient_light
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
            ],
        })
    }

    fn collect_point_lights(scene: &Scene) -> Vec<PointLight> {
        let mut point_lights = Vec::new();
        let mut query = <(Entity, &ComponentPointLight)>::query();
        for (game_object, light) in query
            .iter(&scene.world)
            .filter(|(_, light)| light.active)
            .filter_map(|(entity, light)| {
                scene.game_object_from_entity(*entity).map(|go| (go, light))
            })
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
            .iter(&scene.world)
            .filter(|(_, light)| light.active)
            .filter_map(|(entity, light)| {
                scene.game_object_from_entity(*entity).map(|go| (go, light))
            })
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

    fn collect_ambient_light(scene: &Scene) -> [f32; 4] {
        let mut ambient = [0.0, 0.0, 0.0, 1.0];
        let mut query = <&ComponentAmbientLight>::query();
        for light in query.iter(&scene.world).filter(|light| light.active) {
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
    use crate::component::{ComponentAmbientLight, ComponentDirectionalLight, ComponentPointLight};
    use crate::test_utils::test_scene;
    use egui::Color32;
    use nalgebra_glm::{translation, vec3};

    #[test]
    fn collect_point_lights_includes_active_lights() {
        let mut scene = test_scene();
        let active = scene.create(None, None);
        scene.set_world_transform(active, translation(&vec3(1.0, 2.0, 3.0)));
        scene.add_component(
            active,
            ComponentPointLight {
                radius: 7.5,
                color: Color32::RED,
                intensity: 0.25,
                ..Default::default()
            },
        );

        let inactive = scene.create(None, None);
        scene.add_component(
            inactive,
            ComponentPointLight {
                active: false,
                radius: 99.0,
                color: Color32::BLUE,
                ..Default::default()
            },
        );

        let lights = LightManager::collect_point_lights(&scene);

        assert_eq!(lights.len(), 1);
        assert_eq!(lights[0].position, [1.0, 2.0, 3.0]);
        assert_eq!(lights[0].radius, 7.5);
        assert_eq!(lights[0].color, [1.0, 0.0, 0.0]);
        assert_eq!(lights[0].intensity, 0.25);
    }

    #[test]
    fn collect_directional_lights_includes_active_lights() {
        let mut scene = test_scene();
        let active = scene.create(None, None);
        scene.add_component(
            active,
            ComponentDirectionalLight {
                color: Color32::GREEN,
                intensity: 0.5,
                ..Default::default()
            },
        );

        let inactive = scene.create(None, None);
        scene.add_component(
            inactive,
            ComponentDirectionalLight {
                active: false,
                color: Color32::BLUE,
                ..Default::default()
            },
        );

        let lights = LightManager::collect_directional_lights(&scene);

        assert_eq!(lights.len(), 1);
        assert_eq!(lights[0].direction, [0.0, 0.0, 1.0]);
        assert_eq!(lights[0].color, [0.0, 1.0, 0.0]);
        assert_eq!(lights[0].intensity, 0.5);
    }

    #[test]
    fn collect_ambient_light_sums_active_lights() {
        let mut scene = test_scene();
        let first = scene.create(None, None);
        scene.add_component(
            first,
            ComponentAmbientLight {
                color: Color32::RED,
                intensity: 0.25,
                ..Default::default()
            },
        );
        let second = scene.create(None, None);
        scene.add_component(
            second,
            ComponentAmbientLight {
                color: Color32::BLUE,
                intensity: 0.5,
                ..Default::default()
            },
        );
        let inactive = scene.create(None, None);
        scene.add_component(
            inactive,
            ComponentAmbientLight {
                active: false,
                color: Color32::GREEN,
                intensity: 1.0,
            },
        );

        let ambient = LightManager::collect_ambient_light(&scene);

        assert_eq!(ambient, [0.25, 0.0, 0.5, 1.0]);
    }
}
