use std::default::Default;
use std::path::Path;

use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt;
use egui_wgpu::wgpu::BufferUsages;
use glm::{vec4, Mat4};
use legion::{Entity, IntoQuery};
use rapier3d::pipeline::DebugRenderPipeline;

use crate::assets::mesh::Mesh;
use crate::assets::{Asset, Assets};
use crate::class_registry::ClassRegistry;
use crate::math::Transform;
use crate::physics::PhysicsDebugRenderer;
use crate::render::gizmos::Gizmos;
use crate::render::render_utils::RenderUtils;
use crate::scene::Scene;

use super::buffer::wgpu_buffer_init_desc;
use super::{PipelineOptions, PipelineOptionsBuilder, RenderContext, Shader};

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GizmoInstance {
    pub transform: [[f32; 4]; 4],
    pub color: [f32; 4],
    pub enable_normals: i32,
    pub use_uv_colors: i32,
    pub _padding: [u32; 2],
}

pub struct GizmoRenderer {
    samples: u32,
    circle_list: Vec<GizmoInstance>,
    cube_list: Vec<GizmoInstance>,

    wire_circle_mesh: Mesh,
    wire_cube_mesh: Mesh,
    lines_mesh: Mesh,
    points_mesh: Mesh,

    shader: Shader,
    gizmo_bind_group: wgpu::BindGroup,
    circle_bind_group: wgpu::BindGroup,
    cube_bind_group: wgpu::BindGroup,
    lines_bind_group: wgpu::BindGroup,
    points_bind_group: wgpu::BindGroup,

    circle_instance_buffer: wgpu::Buffer,
    cube_instance_buffer: wgpu::Buffer,
}

impl GizmoRenderer {
    pub fn new(camera_uniform_buffer: &wgpu::Buffer, samples: u32) -> Self {
        let render_state = RenderContext::render_state().unwrap();
        let device = &render_state.device;

        let circle_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("circle_instance_buffer"),
            size: (std::mem::size_of::<GizmoInstance>() * Mesh::MAX_INSTANCES) as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let cube_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cube_instance_buffer"),
            size: (std::mem::size_of::<GizmoInstance>() * Mesh::MAX_INSTANCES) as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let single_instance_buffer = device.create_buffer_init(&wgpu_buffer_init_desc(
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            &[GizmoInstance {
                transform: Mat4::identity().into(),
                color: [1.0; 4],
                enable_normals: 0,
                use_uv_colors: 1,
                _padding: Default::default(),
            }; Mesh::MAX_INSTANCES],
        ));

        let shader = Shader::from_file(&Path::new("assets/shaders/gizmos.wgsl"))
            .unwrap()
            .asset;

        let gizmo_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gizmo_bind_group"),
            layout: &shader.bind_group_layouts[0],
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_uniform_buffer.as_entire_binding(),
            }],
        });

        let lines_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lines_bind_group"),
            layout: &shader.bind_group_layouts[1],
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: single_instance_buffer.as_entire_binding(),
            }],
        });

        let points_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("points_bind_group"),
            layout: &shader.bind_group_layouts[1],
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: single_instance_buffer.as_entire_binding(),
            }],
        });

        let circle_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("circle_bind_group"),
            layout: &shader.bind_group_layouts[1],
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: circle_instance_buffer.as_entire_binding(),
            }],
        });

        let cube_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cube_bind_group"),
            layout: &shader.bind_group_layouts[1],
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: cube_instance_buffer.as_entire_binding(),
            }],
        });

        let renderer = Self {
            samples,
            circle_list: Vec::new(),
            cube_list: Vec::new(),

            wire_circle_mesh: Assets::wire_circle(),
            wire_cube_mesh: Assets::wire_cube(),
            lines_mesh: Mesh::default(),
            points_mesh: Mesh::default(),

            shader,
            gizmo_bind_group,
            circle_bind_group,
            cube_bind_group,
            lines_bind_group,
            points_bind_group,

            circle_instance_buffer,
            cube_instance_buffer,
        };
        renderer
    }

    fn pipeline_options(
        topology: wgpu::PrimitiveTopology,
        target_format: wgpu::TextureFormat,
        samples: u32,
    ) -> PipelineOptions {
        PipelineOptionsBuilder::default()
            .samples(samples)
            .primitive_topology(topology)
            .fragment_targets(vec![Some(RenderUtils::color_alpha_blending(target_format))])
            .build()
    }

    fn clear(&mut self) {
        self.circle_list.clear();
        self.cube_list.clear();
        self.lines_mesh.clear();
        self.points_mesh.clear();
    }

    fn load_buffers(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        queue.write_buffer(
            &self.circle_instance_buffer,
            0 as wgpu::BufferAddress,
            bytemuck::cast_slice(
                &self.circle_list[..self.circle_list.len().min(Mesh::MAX_INSTANCES)],
            ),
        );
        queue.write_buffer(
            &self.cube_instance_buffer,
            0 as wgpu::BufferAddress,
            bytemuck::cast_slice(&self.cube_list[..self.cube_list.len().min(Mesh::MAX_INSTANCES)]),
        );
        RenderUtils::rebuild_mesh_data(device, queue, &mut self.wire_circle_mesh);
        RenderUtils::rebuild_mesh_data(device, queue, &mut self.wire_cube_mesh);
        self.lines_mesh.rebuild_mesh_data(device);
        self.points_mesh.rebuild_mesh_data(device);
    }

    pub fn gizmos<'a>(&'a mut self, camera_transform: &'a Transform) -> Gizmos {
        self.clear();
        Gizmos {
            camera_transform,
            color: vec4(1.0, 1.0, 1.0, 1.0),
            circle_list: &mut self.circle_list,
            cube_list: &mut self.cube_list,
            lines_mesh: &mut self.lines_mesh,
            points_mesh: &mut self.points_mesh,
        }
    }

    pub fn draw_gizmos(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera_transform: &Transform,
        scene: &Scene,
        physics_debug_pipeline: Option<&mut DebugRenderPipeline>,
    ) {
        {
            let mut gizmos = self.gizmos(camera_transform);
            let mut query = <Entity>::query();
            let world = &scene.world;
            for entity in query.iter(world) {
                if let Some(game_object) = scene.get_game_object_from_entity(*entity) {
                    for (_, comp) in ClassRegistry::get().components() {
                        if let Some(entry) = scene.entry(game_object) {
                            if let Some(instance) = comp.get_instance(&entry) {
                                instance.draw_gizmos(scene, game_object, &mut gizmos);
                            }
                        }
                    }
                }
            }
            if let Some(physics_debug_pipeline) = physics_debug_pipeline {
                let mut physics_debug_render: PhysicsDebugRenderer = gizmos.into();
                physics_debug_pipeline.render(
                    &mut physics_debug_render,
                    &scene.physics.bodies,
                    &scene.physics.colliders,
                    &scene.physics.impulse_joints,
                    &scene.physics.multibody_joints,
                    &scene.physics.narrow_phase,
                );
            }
        }
        self.load_buffers(device, queue);
    }

    pub fn render_gizmos<'a>(
        &'a mut self,
        target_format: wgpu::TextureFormat,
        render_pass: &mut wgpu::RenderPass<'a>,
    ) {
        let circle_options = Self::pipeline_options(
            wgpu::PrimitiveTopology::LineStrip,
            target_format,
            self.samples,
        );
        let cube_options = Self::pipeline_options(
            wgpu::PrimitiveTopology::LineList,
            target_format,
            self.samples,
        );
        let line_options = Self::pipeline_options(
            wgpu::PrimitiveTopology::LineList,
            target_format,
            self.samples,
        );
        let point_options = Self::pipeline_options(
            wgpu::PrimitiveTopology::PointList,
            target_format,
            self.samples,
        );
        self.shader.build_pipeline(&circle_options);
        self.shader.build_pipeline(&cube_options);
        self.shader.build_pipeline(&line_options);
        self.shader.build_pipeline(&point_options);
        if !self.circle_list.is_empty() {
            if let Some(pipeline) = self.shader.get_pipeline(&circle_options) {
                render_pass.set_pipeline(pipeline);
                render_pass.set_bind_group(0, &self.gizmo_bind_group, &[]);

                RenderUtils::bind_mesh_buffers(render_pass, &self.wire_circle_mesh);
                render_pass.set_bind_group(1, &self.circle_bind_group, &[]);
                render_pass.draw(
                    0..(self.wire_circle_mesh.vertices.len() as u32),
                    0..(self.circle_list.len() as u32),
                );
            }
        }

        if !self.cube_list.is_empty() {
            if let Some(pipeline) = self.shader.get_pipeline(&cube_options) {
                render_pass.set_pipeline(pipeline);
                render_pass.set_bind_group(0, &self.gizmo_bind_group, &[]);

                RenderUtils::bind_mesh_buffers(render_pass, &self.wire_cube_mesh);
                render_pass.set_bind_group(1, &self.cube_bind_group, &[]);
                render_pass.draw_indexed(
                    0..(self.wire_cube_mesh.indices.len() as u32),
                    0,
                    0..(self.cube_list.len() as u32),
                );
            }
        }

        if !self.lines_mesh.vertices.is_empty() {
            if let Some(pipeline) = self.shader.get_pipeline(&line_options) {
                render_pass.set_pipeline(pipeline);
                render_pass.set_bind_group(0, &self.gizmo_bind_group, &[]);

                RenderUtils::bind_mesh_buffers(render_pass, &self.lines_mesh);
                render_pass.set_bind_group(1, &self.lines_bind_group, &[]);
                render_pass.draw(0..(self.lines_mesh.vertices.len() as u32), 0..1);
            }
        }

        if !self.points_mesh.vertices.is_empty() {
            if let Some(pipeline) = self.shader.get_pipeline(&point_options) {
                render_pass.set_pipeline(pipeline);
                render_pass.set_bind_group(0, &self.gizmo_bind_group, &[]);

                RenderUtils::bind_mesh_buffers(render_pass, &self.points_mesh);
                render_pass.set_bind_group(1, &self.points_bind_group, &[]);
                render_pass.draw(0..(self.points_mesh.vertices.len() as u32), 0..1);
            }
        }
    }
}
