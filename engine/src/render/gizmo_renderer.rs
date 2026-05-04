use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt;
use egui_wgpu::wgpu::BufferUsages;
use legion::{Entity, IntoQuery};
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec4, Mat4};
use rapier3d::pipeline::DebugRenderPipeline;
use std::default::Default;
use std::path::Path;

use crate::assets::mesh::Mesh;
use crate::assets::Asset;
use crate::class_registry::ComponentRegistry;
use crate::component::{ComponentMesh, ComponentSkinnedMesh};
use crate::context::ReadOnlyAssetContext;
use crate::core::ReadOnlyRef;
use crate::math::Transform;
use crate::physics::PhysicsDebugRenderer;
use crate::render::gizmos::Gizmos;
use crate::render::render_utils::RenderUtils;
use crate::scene::Scene;
use uuid::Uuid;

use super::buffer::wgpu_buffer_init_desc;
use super::{PipelineOptions, Shader};

/// Per-instance draw data for gizmo rendering.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GizmoInstance {
    /// Instance transform.
    pub transform: [[f32; 4]; 4],
    /// Instance color.
    pub color: [f32; 4],
    /// Whether circle gizmos should face the camera.
    pub enable_normals: i32,
    /// Whether vertex UVs should be interpreted as colors.
    pub use_uv_colors: i32,
    /// Padding for alignment.
    pub _padding: [u32; 2],
}

/// Collects and renders debug gizmos for scenes and physics.
pub struct GizmoRenderer {
    component_registry: ReadOnlyRef<ComponentRegistry>,

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

    highlighted_game_objects: Vec<Uuid>,
}

impl GizmoRenderer {
    /// Creates a gizmo renderer bound to `camera_uniform_buffer`.
    pub fn new(
        game: &ReadOnlyAssetContext,
        camera_uniform_buffer: &wgpu::Buffer,
        samples: u32,
    ) -> Self {
        let render_state = game.render_context.render_state();
        let device = &render_state.device;

        let circle_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("circle_instance_buffer"),
            size: (size_of::<GizmoInstance>() * Mesh::MAX_INSTANCES) as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let cube_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cube_instance_buffer"),
            size: (size_of::<GizmoInstance>() * Mesh::MAX_INSTANCES) as u64,
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

        let shader = Shader::from_file(game, Path::new("assets/shaders/gizmos.wgsl"))
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

            wire_circle_mesh: game.registries.assets.read().wire_circle(),
            wire_cube_mesh: game.registries.assets.read().wire_cube(),
            lines_mesh: Mesh::new(&game.render_context),
            points_mesh: Mesh::new(&game.render_context),

            shader,
            gizmo_bind_group,
            circle_bind_group,
            cube_bind_group,
            lines_bind_group,
            points_bind_group,

            circle_instance_buffer,
            cube_instance_buffer,

            component_registry: game.registries.components.clone(),
            highlighted_game_objects: Vec::new(),
        };
        renderer
    }

    fn pipeline_options(
        &self,
        topology: wgpu::PrimitiveTopology,
        target_format: wgpu::TextureFormat,
        samples: u32,
    ) -> PipelineOptions {
        PipelineOptions::builder()
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

    /// Returns a gizmo command recorder for the current frame.
    pub fn gizmos<'a>(&'a mut self, camera_transform: &'a Transform) -> Gizmos<'a> {
        self.clear();
        Gizmos {
            camera_transform,
            color: vec4(1.0, 1.0, 1.0, 1.0),
            depth_test_enabled: true,
            circle_list: &mut self.circle_list,
            cube_list: &mut self.cube_list,
            lines_mesh: &mut self.lines_mesh,
            points_mesh: &mut self.points_mesh,
        }
    }

    /// Replaces the current list of highlighted game objects.
    pub fn set_highlighted_game_objects(
        &mut self,
        highlighted_game_objects: impl IntoIterator<Item = Uuid>,
    ) {
        self.highlighted_game_objects = highlighted_game_objects.into_iter().collect();
    }

    /// Collects gizmos from components and optional physics debug rendering.
    pub fn draw_gizmos(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera_transform: &Transform,
        scene: &Scene,
        physics_debug_pipeline: Option<&mut DebugRenderPipeline>,
    ) {
        {
            let registry_ref = self.component_registry.clone();
            let mut gizmos;
            {
                let registry = registry_ref.read();
                gizmos = self.gizmos(camera_transform);
                let mut query = <Entity>::query();
                let world = &scene.world;
                for entity in query.iter(world) {
                    if let Some(game_object) = scene.game_object_from_entity(*entity) {
                        for (_, comp) in registry.components() {
                            if let Some(entry) = scene.entry(game_object) {
                                if let Some(instance) = comp.get_instance(&entry) {
                                    instance.draw_gizmos(scene, game_object, &mut gizmos);
                                }
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
        self.draw_highlights(camera_transform, scene);
        self.load_buffers(device, queue);
    }

    fn draw_highlights(&mut self, camera_transform: &Transform, scene: &Scene) {
        if self.highlighted_game_objects.is_empty() {
            return;
        }

        let mut gizmos = Gizmos {
            camera_transform,
            color: vec4(1.0, 0.84, 0.0, 1.0),
            depth_test_enabled: true,
            circle_list: &mut self.circle_list,
            cube_list: &mut self.cube_list,
            lines_mesh: &mut self.lines_mesh,
            points_mesh: &mut self.points_mesh,
        };

        for highlighted_id in &self.highlighted_game_objects {
            let Some(game_object) = scene.find(*highlighted_id) else {
                continue;
            };
            let Some(entry) = scene.entry(game_object) else {
                continue;
            };
            if let Ok(component) = entry.get_component::<ComponentMesh>() {
                if let Some(mesh_ref) = component.mesh.get_ref(scene.registries()) {
                    Self::draw_mesh_highlight(
                        &mut gizmos,
                        scene.world_transform(game_object),
                        &mesh_ref.read(),
                    );
                }
                continue;
            }
            if let Ok(component) = entry.get_component::<ComponentSkinnedMesh>() {
                if let Some(mesh_ref) = component.mesh.get_ref(scene.registries()) {
                    Self::draw_mesh_highlight(
                        &mut gizmos,
                        scene.world_transform(game_object),
                        &mesh_ref.read(),
                    );
                }
            }
        }
    }

    fn draw_mesh_highlight(gizmos: &mut Gizmos<'_>, transform: Transform, mesh: &Mesh) {
        let Some((min, max)) = mesh.local_bounds() else {
            return;
        };
        let center = (min + max) * 0.5;
        let size = max - min;
        let cube_transform = transform.matrix()
            * crate::math::compose_transform(&center, &UnitQuaternion::identity(), &size);
        gizmos.wire_cube_transform(cube_transform);
    }

    /// Renders all collected gizmos into the current render pass.
    pub fn render_gizmos<'a>(
        &'a mut self,
        target_format: wgpu::TextureFormat,
        render_pass: &mut wgpu::RenderPass<'a>,
    ) {
        let circle_options = self.pipeline_options(
            wgpu::PrimitiveTopology::LineStrip,
            target_format,
            self.samples,
        );
        let cube_options = self.pipeline_options(
            wgpu::PrimitiveTopology::LineList,
            target_format,
            self.samples,
        );
        let line_options = self.pipeline_options(
            wgpu::PrimitiveTopology::LineList,
            target_format,
            self.samples,
        );
        let point_options = self.pipeline_options(
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
