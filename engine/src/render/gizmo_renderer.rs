use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt;
use egui_wgpu::wgpu::BufferUsages;
use image::{imageops, RgbaImage};
use legion::{Entity, IntoQuery};
use nalgebra_glm::{vec4, Mat4};
use rapier3d::pipeline::DebugRenderPipeline;
use std::default::Default;
use std::path::Path;

use crate::assets::mesh::Mesh;
use crate::assets::Asset;
use crate::class_registry::ComponentRegistry;
use crate::context::ReadOnlyAssetContext;
use crate::core::ReadOnlyRef;
use crate::math::Transform;
use crate::physics::PhysicsDebugRenderer;
use crate::render::gizmos::Gizmos;
use crate::render::render_utils::RenderUtils;
use crate::scene::Scene;

use super::buffer::wgpu_buffer_init_desc;
use super::{PipelineOptions, Shader};

const HIDDEN_GIZMO_OPACITY: f32 = 0.35;
const PHOSPHOR_ICON_ATLAS_PNG: &[u8] =
    include_bytes!("../../../resources/icons/phosphor_regular.png");
const PHOSPHOR_CAMERA_FILL_RECT: (u32, u32, u32, u32) = (4622, 794, 128, 128);
const PHOSPHOR_LIGHTBULB_FILL_RECT: (u32, u32, u32, u32) = (1850, 2774, 128, 128);

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
    icons_mesh: Mesh,

    shader: Shader,
    icon_shader: Shader,
    gizmo_bind_group: wgpu::BindGroup,
    icon_texture: IconTexture,
    circle_bind_group: wgpu::BindGroup,
    cube_bind_group: wgpu::BindGroup,
    lines_bind_group: wgpu::BindGroup,
    points_bind_group: wgpu::BindGroup,

    circle_instance_buffer: wgpu::Buffer,
    cube_instance_buffer: wgpu::Buffer,
}

struct IconTexture {
    _texture: wgpu::Texture,
    _view: wgpu::TextureView,
    _sampler: wgpu::Sampler,
    bind_group: wgpu::BindGroup,
}

impl GizmoRenderer {
    /// Creates a gizmo renderer bound to `camera_uniform_buffer`.
    pub fn new(
        game: &ReadOnlyAssetContext,
        camera_uniform_buffer: &wgpu::Buffer,
        samples: u32,
    ) -> Self {
        let device = game.render_context.device();
        let queue = game.render_context.queue();

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

        let shader = Self::load_shader(game, Path::new("shaders/gizmos.wgsl"));
        let icon_shader = Self::load_shader(game, Path::new("shaders/gizmo_icons.wgsl"));

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

        let icon_texture =
            Self::create_icon_texture(device, queue, &icon_shader.bind_group_layouts[1]);

        let renderer = Self {
            samples,
            circle_list: Vec::new(),
            cube_list: Vec::new(),

            wire_circle_mesh: game.registries.assets.read().wire_circle(),
            wire_cube_mesh: game.registries.assets.read().wire_cube(),
            lines_mesh: Mesh::new(&game.render_context),
            points_mesh: Mesh::new(&game.render_context),
            icons_mesh: Mesh::new(&game.render_context),

            shader,
            icon_shader,
            gizmo_bind_group,
            icon_texture,
            circle_bind_group,
            cube_bind_group,
            lines_bind_group,
            points_bind_group,

            circle_instance_buffer,
            cube_instance_buffer,

            component_registry: game.registries.components.clone(),
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
        self.icons_mesh.clear();
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
        self.icons_mesh.rebuild_mesh_data(device);
    }

    /// Returns a gizmo command recorder for the current frame.
    pub fn gizmos<'a>(&'a mut self, camera_transform: &'a Transform) -> Gizmos<'a> {
        self.clear();
        Gizmos {
            camera_transform,
            color: vec4(1.0, 1.0, 1.0, 1.0),
            opacity: 1.0,
            depth_test_enabled: true,
            circle_list: &mut self.circle_list,
            cube_list: &mut self.cube_list,
            lines_mesh: &mut self.lines_mesh,
            points_mesh: &mut self.points_mesh,
            icons_mesh: &mut self.icons_mesh,
        }
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
                        let opacity = if scene.is_visible_in_hierarchy(game_object) {
                            1.0
                        } else {
                            HIDDEN_GIZMO_OPACITY
                        };
                        gizmos.set_opacity(opacity);
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
                gizmos.set_opacity(1.0);
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
        let mut icon_depth = RenderUtils::depth_default(wgpu::TextureFormat::Depth32Float);
        icon_depth.depth_write_enabled = false;
        let icon_options = PipelineOptions::builder()
            .samples(self.samples)
            .cull_mode(None)
            .depth_stencil(Some(icon_depth))
            .fragment_targets(vec![Some(RenderUtils::color_alpha_blending(target_format))])
            .build();
        self.shader.build_pipeline(&circle_options);
        self.shader.build_pipeline(&cube_options);
        self.shader.build_pipeline(&line_options);
        self.shader.build_pipeline(&point_options);
        self.icon_shader.build_pipeline(&icon_options);
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

        if !self.icons_mesh.indices.is_empty() {
            if let Some(pipeline) = self.icon_shader.get_pipeline(&icon_options) {
                render_pass.set_pipeline(pipeline);
                render_pass.set_bind_group(0, &self.gizmo_bind_group, &[]);
                render_pass.set_bind_group(1, &self.icon_texture.bind_group, &[]);

                RenderUtils::bind_mesh_buffers(render_pass, &self.icons_mesh);
                render_pass.draw_indexed(0..(self.icons_mesh.indices.len() as u32), 0, 0..1);
            }
        }
    }

    fn create_icon_texture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
    ) -> IconTexture {
        let (pixels, width, height) = Self::icon_atlas_pixels();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("gizmo_icon_atlas"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("gizmo_icon_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gizmo_icon_texture_bind_group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        IconTexture {
            _texture: texture,
            _view: view,
            _sampler: sampler,
            bind_group,
        }
    }

    fn icon_atlas_pixels() -> (Vec<u8>, u32, u32) {
        let atlas = image::load_from_memory(PHOSPHOR_ICON_ATLAS_PNG)
            .unwrap_or_else(|err| panic!("failed to decode Phosphor gizmo icon atlas: {err}"))
            .to_rgba8();
        let camera = Self::crop_icon(&atlas, PHOSPHOR_CAMERA_FILL_RECT);
        let light = Self::crop_icon(&atlas, PHOSPHOR_LIGHTBULB_FILL_RECT);
        let width = camera.width() + light.width();
        let height = camera.height().max(light.height());
        let mut pixels = vec![0; (width * height * 4) as usize];
        Self::copy_icon(&mut pixels, width, &camera, 0, 0);
        Self::copy_icon(&mut pixels, width, &light, camera.width(), 0);
        (pixels, width, height)
    }

    fn crop_icon(atlas: &RgbaImage, rect: (u32, u32, u32, u32)) -> RgbaImage {
        let (x, y, width, height) = rect;
        imageops::crop_imm(atlas, x, y, width, height).to_image()
    }

    fn copy_icon(
        pixels: &mut [u8],
        atlas_width: u32,
        icon: &RgbaImage,
        x_offset: u32,
        y_offset: u32,
    ) {
        for y in 0..icon.height() {
            for x in 0..icon.width() {
                let src = icon.get_pixel(x, y).0;
                let dst = (((y + y_offset) * atlas_width + x + x_offset) * 4) as usize;
                pixels[dst..dst + 4].copy_from_slice(&src);
            }
        }
    }

    fn load_shader(game: &ReadOnlyAssetContext, relative_path: &Path) -> Shader {
        let asset_paths = game.registries.assets.read().asset_paths().clone();
        for asset_path in asset_paths {
            let path = asset_path.join(relative_path);
            if path.exists() {
                return Shader::from_file(game, &path).unwrap().asset;
            }
        }
        panic!("missing gizmo shader {}", relative_path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::test_asset_context_with_assets;
    use std::path::PathBuf;

    fn assets_path() -> PathBuf {
        let assets_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets");
        dunce::canonicalize(assets_path).expect("assets dir not found")
    }

    #[test]
    fn gizmo_icon_shader_builds_pipeline() {
        let context = test_asset_context_with_assets(vec![assets_path()]);
        let read_only_context = context.lock_read();
        let mut shader = Shader::from_file(
            &read_only_context,
            &assets_path().join("shaders/gizmo_icons.wgsl"),
        )
        .expect("icon shader should load")
        .asset;
        let mut depth_stencil = RenderUtils::depth_default(wgpu::TextureFormat::Depth32Float);
        depth_stencil.depth_write_enabled = false;
        let icon_options = PipelineOptions::builder()
            .cull_mode(None)
            .depth_stencil(Some(depth_stencil))
            .fragment_targets(vec![Some(RenderUtils::color_alpha_blending(
                wgpu::TextureFormat::Rgba16Float,
            ))])
            .build();

        shader.build_pipeline(&icon_options);

        assert!(shader.get_pipeline(&icon_options).is_some());
    }
}
