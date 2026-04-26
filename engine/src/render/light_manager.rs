use crate::component::{ComponentDirectionalLight, ComponentPointLight};
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
    _padding: f32,
}

#[repr(C)]
#[derive(Default, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct DirectionalLight {
    direction: [f32; 3],
    _padding: f32,
    color: [f32; 3],
    _padding2: f32,
}

pub struct LightManager {
    point_light_storage_buffer: ResizableBuffer,
    directional_light_storage_buffer: ResizableBuffer,
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
                radius: light.radius,
                position: scene.world_transform(game_object).position.into(),
                ..Default::default()
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
                direction: scene
                    .world_transform(game_object)
                    .transform_direction(&Vec3::z_axis())
                    .into(),
                ..Default::default()
            })
        }
        directional_lights
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::{ComponentDirectionalLight, ComponentPointLight};
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
            },
        );

        let lights = LightManager::collect_point_lights(&scene);

        assert_eq!(lights.len(), 1);
        assert_eq!(lights[0].position, [1.0, 2.0, 3.0]);
        assert_eq!(lights[0].radius, 7.5);
        assert_eq!(lights[0].color, [1.0, 0.0, 0.0]);
    }

    #[test]
    fn collect_directional_lights_includes_active_lights() {
        let mut scene = test_scene();
        let active = scene.create(None, None);
        scene.add_component(
            active,
            ComponentDirectionalLight {
                color: Color32::GREEN,
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
    }
}
