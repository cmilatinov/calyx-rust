use crate::assets::mesh::Mesh;
use crate::assets::{mesh, Assets};
use crate::class_registry::ClassRegistry;
use crate::math::Transform;
use crate::render::buffer::{wgpu_buffer_init_desc, BufferLayout, ResizableBuffer};
use crate::render::gizmos::Gizmos;
use crate::render::render_utils::RenderUtils;
use crate::render::CameraUniform;
use crate::scene::Scene;
use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt;
use egui_wgpu::wgpu::BufferUsages;
use glm::{vec4, Mat4, Vec4};
use legion::{Entity, EntityStore, IntoQuery};
use std::default::Default;
use std::num::NonZeroU64;
use std::ops::Deref;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GizmoInstance {
    pub color: [f32; 4],
    pub enable_normals: i32,
    pub use_uv_colors: i32,
}

impl GizmoInstance {
    const ATTRIBUTES: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![
        10 => Float32x4,
        11 => Sint32,
        12 => Sint32
    ];
}

impl BufferLayout for GizmoInstance {
    const ATTRIBS: &'static [wgpu::VertexAttribute] = &GizmoInstance::ATTRIBUTES;
}

pub struct GizmoRenderer {
    circle_list: Vec<GizmoInstance>,
    cube_list: Vec<GizmoInstance>,

    wire_circle_mesh: Mesh,
    wire_cube_mesh: Mesh,
    lines_mesh: Mesh,
    points_mesh: Mesh,

    gizmo_bind_group: wgpu::BindGroup,
    gizmo_pipeline_line_list: wgpu::RenderPipeline,
    gizmo_pipeline_line_strip: wgpu::RenderPipeline,
    gizmo_pipeline_point_list: wgpu::RenderPipeline,

    circle_instance_buffer: ResizableBuffer,
    cube_instance_buffer: ResizableBuffer,
    lines_instance_buffer: wgpu::Buffer,
    points_instance_buffer: wgpu::Buffer,
}

impl GizmoRenderer {
    pub fn new(
        cc: &eframe::CreationContext,
        camera_uniform_buffer: &wgpu::Buffer,
        samples: u32,
    ) -> Self {
        let render_state = cc.wgpu_render_state.as_ref().unwrap();
        let device = &render_state.device;
        let queue = &render_state.queue;

        let circle_instance_buffer =
            ResizableBuffer::new(BufferUsages::VERTEX | BufferUsages::COPY_DST);
        let cube_instance_buffer =
            ResizableBuffer::new(BufferUsages::VERTEX | BufferUsages::COPY_DST);
        let instance = GizmoInstance {
            color: Vec4::default().into(),
            enable_normals: 0,
            use_uv_colors: 1,
        };
        let lines_instance_buffer =
            device.create_buffer_init(&wgpu_buffer_init_desc(BufferUsages::VERTEX, &[instance]));
        let points_instance_buffer =
            device.create_buffer_init(&wgpu_buffer_init_desc(BufferUsages::VERTEX, &[instance]));

        let gizmo_shader =
            device.create_shader_module(wgpu::include_wgsl!("../../../assets/shaders/gizmos.wgsl"));

        let gizmo_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("gizmo_bind_group_layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(
                            std::mem::size_of::<CameraUniform>() as u64
                        ),
                    },
                    count: None,
                }],
            });

        let gizmo_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gizmo_bind_group"),
            layout: &gizmo_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_uniform_buffer.as_entire_binding(),
            }],
        });

        let gizmo_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("gizmo_pipeline_layout"),
                bind_group_layouts: &[&gizmo_bind_group_layout],
                push_constant_ranges: &[],
            });

        let gizmo_pipeline_line_list = Self::create_pipeline(
            render_state,
            "gizmo_pipeline_line_list",
            &gizmo_pipeline_layout,
            &gizmo_shader,
            wgpu::PrimitiveTopology::LineList,
            samples,
        );

        let gizmo_pipeline_line_strip = Self::create_pipeline(
            render_state,
            "gizmo_pipeline_line_strip",
            &gizmo_pipeline_layout,
            &gizmo_shader,
            wgpu::PrimitiveTopology::LineStrip,
            samples,
        );

        let gizmo_pipeline_point_list = Self::create_pipeline(
            render_state,
            "gizmo_pipeline_point_list",
            &gizmo_pipeline_layout,
            &gizmo_shader,
            wgpu::PrimitiveTopology::PointList,
            samples,
        );

        let mut renderer = Self {
            circle_list: Vec::new(),
            cube_list: Vec::new(),

            wire_circle_mesh: Assets::wire_circle(),
            wire_cube_mesh: Assets::wire_cube(),
            lines_mesh: Mesh::default(),
            points_mesh: Mesh::default(),

            gizmo_bind_group,
            gizmo_pipeline_line_list,
            gizmo_pipeline_line_strip,
            gizmo_pipeline_point_list,

            circle_instance_buffer,
            cube_instance_buffer,
            lines_instance_buffer,
            points_instance_buffer,
        };
        renderer.lines_mesh.instances.push(Mat4::identity().into());
        renderer.lines_mesh.rebuild_instance_data(device, queue);
        renderer.points_mesh.instances.push(Mat4::identity().into());
        renderer.points_mesh.rebuild_instance_data(device, queue);
        renderer
    }

    fn create_pipeline(
        render_state: &egui_wgpu::RenderState,
        name: &str,
        layout: &wgpu::PipelineLayout,
        shader: &wgpu::ShaderModule,
        topology: wgpu::PrimitiveTopology,
        samples: u32,
    ) -> wgpu::RenderPipeline {
        let device = &render_state.device;
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(name),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: "vs_main",
                buffers: &[
                    mesh::Vertex::layout(wgpu::VertexStepMode::Vertex),
                    mesh::Instance::layout(wgpu::VertexStepMode::Instance),
                    GizmoInstance::layout(wgpu::VertexStepMode::Instance),
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: "fs_main",
                targets: &[Some(RenderUtils::color_alpha_blending(render_state))],
            }),
            primitive: RenderUtils::primitive_default(topology),
            depth_stencil: Some(RenderUtils::depth_default()),
            multisample: RenderUtils::multisample_default(samples),
            multiview: None,
        })
    }

    fn clear(&mut self) {
        self.circle_list.clear();
        self.cube_list.clear();
        self.wire_circle_mesh.instances.clear();
        self.wire_cube_mesh.instances.clear();
        self.lines_mesh.clear();
        self.points_mesh.clear();
    }

    fn load_buffers(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.circle_instance_buffer
            .write_buffer(device, queue, self.circle_list.as_slice());
        self.cube_instance_buffer
            .write_buffer(device, queue, self.cube_list.as_slice());
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
            wire_circle_mesh: &mut self.wire_circle_mesh,
            wire_cube_mesh: &mut self.wire_cube_mesh,
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
    ) {
        {
            let mut gizmos = self.gizmos(camera_transform);
            let mut query = <Entity>::query();
            let world = scene.world();
            for entity in query.iter(world.deref()) {
                let node = scene.get_node(*entity);
                for comp in ClassRegistry::get().components().iter() {
                    if let Ok(entry) = world.entry_ref(*entity) {
                        if let Some(instance) = comp.get_instance(&entry) {
                            instance.draw_gizmos(scene, node, &mut gizmos);
                        }
                    }
                }
            }
        }
        self.load_buffers(device, queue);
    }

    pub fn render_gizmos<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>) {
        if !self.wire_circle_mesh.instances.is_empty() {
            render_pass.set_pipeline(&self.gizmo_pipeline_line_strip);
            render_pass.set_bind_group(0, &self.gizmo_bind_group, &[]);

            RenderUtils::bind_mesh_buffers(render_pass, &self.wire_circle_mesh);
            render_pass
                .set_vertex_buffer(2, self.circle_instance_buffer.get_wgpu_buffer().slice(..));
            render_pass.draw(
                0..(self.wire_circle_mesh.vertices.len() as u32),
                0..(self.wire_circle_mesh.instances.len() as u32),
            );
        }

        if !self.wire_cube_mesh.instances.is_empty() {
            render_pass.set_pipeline(&self.gizmo_pipeline_line_list);
            render_pass.set_bind_group(0, &self.gizmo_bind_group, &[]);

            RenderUtils::bind_mesh_buffers(render_pass, &self.wire_cube_mesh);
            render_pass.set_vertex_buffer(2, self.cube_instance_buffer.get_wgpu_buffer().slice(..));
            render_pass.draw_indexed(
                0..(self.wire_cube_mesh.indices.len() as u32),
                0,
                0..(self.wire_cube_mesh.instances.len() as u32),
            );
        }

        if !self.lines_mesh.vertices.is_empty() {
            render_pass.set_pipeline(&self.gizmo_pipeline_line_list);
            render_pass.set_bind_group(0, &self.gizmo_bind_group, &[]);

            RenderUtils::bind_mesh_buffers(render_pass, &self.lines_mesh);
            render_pass.set_vertex_buffer(2, self.lines_instance_buffer.slice(..));
            render_pass.draw(0..(self.lines_mesh.vertices.len() as u32), 0..1);
        }

        if !self.points_mesh.vertices.is_empty() {
            render_pass.set_pipeline(&self.gizmo_pipeline_point_list);
            render_pass.set_bind_group(0, &self.gizmo_bind_group, &[]);

            RenderUtils::bind_mesh_buffers(render_pass, &self.points_mesh);
            render_pass.set_vertex_buffer(2, self.points_instance_buffer.slice(..));
            render_pass.draw(0..(self.points_mesh.vertices.len() as u32), 0..1);
        }
    }
}
