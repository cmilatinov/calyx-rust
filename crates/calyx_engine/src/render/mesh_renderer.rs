use crate::assets::material::MaterialBindGroupCacheKey;
use crate::assets::texture::Texture;
use crate::assets::AssetId;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::asset_render_state::LockedAssetRenderState;
use crate::render::render_utils::RenderUtils;
use crate::render::{
    CachedBindGroup, GizmoRenderer, LightManager, PipelineOptions, SceneRendererError, Shader,
    ShadowBindings,
};
use egui::Color32;
use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt;
use std::collections::{HashMap, HashSet};
use std::ops::Range;

#[repr(C)]
#[derive(Default, Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct EnvironmentUniform {
    sky_light: [f32; 4],
    ambient_light: [f32; 4],
}

/// One draw of the scene's draw list: `instances` of a mesh's instance
/// buffer, drawn with one shader and material. The scene pass, the object id
/// pass and the shadow pass all draw from the same list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DrawBatch {
    pub shader_id: AssetId,
    pub mat_id: AssetId,
    pub mesh_id: AssetId,
    pub instances: Range<u32>,
    /// Whether the batch's meshes draw into shadow maps.
    pub cast_shadows: bool,
}

/// What the light storage bind group binds: the scene shader's group 2
/// layout, the light buffer, then the shadow data buffer, cascade and atlas
/// views and sampler.
type LightStorageBindGroupKey = (
    wgpu::BindGroupLayout,
    wgpu::Buffer,
    wgpu::Buffer,
    [wgpu::TextureView; 2],
    wgpu::Sampler,
);

pub struct MeshRenderer {
    scene_shader: Ref<Shader>,
    object_id_shader: Ref<Shader>,
    environment_uniform_buffer: wgpu::Buffer,
    material_bind_group_cache: HashMap<AssetId, CachedMaterialBindGroups>,
    scene_bind_group: CachedBindGroup<SceneBindGroupKey>,
    light_storage_bind_group: CachedBindGroup<LightStorageBindGroupKey>,
    object_id_camera_bind_group: CachedBindGroup<(wgpu::BindGroupLayout, wgpu::Buffer)>,
}

/// The layout and resources the scene bind group binds: the camera buffer,
/// then the irradiance, prefilter and BRDF maps' views and samplers.
type SceneBindGroupKey = (
    wgpu::BindGroupLayout,
    wgpu::Buffer,
    [wgpu::TextureView; 3],
    [wgpu::Sampler; 3],
);

/// Each material's bind groups, by material, then by group index.
pub(crate) type MaterialBindGroups = HashMap<AssetId, HashMap<u32, wgpu::BindGroup>>;

struct CachedMaterialBindGroups {
    key: MaterialBindGroupCacheKey,
    groups: HashMap<u32, wgpu::BindGroup>,
}

/// The bind groups every PBR lighting pass binds, forward or deferred: group
/// 0, the camera and the sky light's maps, and group 2, the lights and the
/// shadow maps (see `shaders/common/pbr_lighting.wgsl`).
#[derive(Clone)]
pub(crate) struct LightingBindGroups {
    pub scene: wgpu::BindGroup,
    pub lights: wgpu::BindGroup,
}

/// Which draws of the draw list the forward pass draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ForwardDraws {
    /// Every draw, over cleared color and depth: the forward path.
    All,
    /// Every draw whose shader is not `deferred_shader`, over the image and
    /// depth the deferred lighting pass left: the deferred path's forward-only
    /// draws.
    AfterDeferred { deferred_shader: AssetId },
}

impl ForwardDraws {
    /// Whether the pass draws the batches of `shader_id`.
    pub fn includes(self, shader_id: AssetId) -> bool {
        match self {
            ForwardDraws::All => true,
            ForwardDraws::AfterDeferred { deferred_shader } => shader_id != deferred_shader,
        }
    }
}

pub struct MeshRenderTargets<'a> {
    pub color: &'a Texture,
    pub depth: &'a Texture,
}

pub struct MeshRenderDefaults<'a> {
    pub black_texture_2d: &'a Texture,
    pub black_texture_cube: &'a Texture,
}

impl MeshRenderer {
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        Ok(Self {
            scene_shader: SceneRendererError::load::<Shader>(context, "shaders/mesh/pbr")?,
            object_id_shader: SceneRendererError::load::<Shader>(
                context,
                "shaders/mesh/object_id",
            )?,
            environment_uniform_buffer: context.render_context.device().create_buffer_init(
                &wgpu::util::BufferInitDescriptor {
                    label: Some("environment_uniform_buffer"),
                    contents: bytemuck::cast_slice(&[EnvironmentUniform::default()]),
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                },
            ),
            material_bind_group_cache: Default::default(),
            scene_bind_group: Default::default(),
            light_storage_bind_group: Default::default(),
            object_id_camera_bind_group: Default::default(),
        })
    }

    /// The PBR shader, whose draws the deferred path shades from the
    /// G-buffer.
    pub fn scene_shader_id(&self) -> AssetId {
        self.scene_shader.id()
    }

    /// The frame's lighting bind groups, for the forward PBR pass and the
    /// deferred lighting pass alike: the camera, the sky light's maps
    /// (`skybox_id`'s, or black without one) and the environment uniform,
    /// then the lights and the shadow maps. Each is reused from earlier
    /// frames while it binds the same resources.
    #[allow(clippy::too_many_arguments)]
    pub fn lighting_bind_groups(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        assets: &LockedAssetRenderState,
        defaults: MeshRenderDefaults<'_>,
        light_manager: &LightManager,
        shadows: &ShadowBindings<'_>,
        camera_uniform_buffer: &wgpu::Buffer,
        skybox_id: Option<AssetId>,
        sky_light_intensity: f32,
        ambient_light: [f32; 4],
    ) -> LightingBindGroups {
        let (irradiance_map, prefilter_map, brdf_map) = skybox_id
            .and_then(|id| assets.skybox(id)?.environment_maps())
            .unwrap_or((
                defaults.black_texture_cube,
                defaults.black_texture_cube,
                defaults.black_texture_2d,
            ));
        queue.write_buffer(
            &self.environment_uniform_buffer,
            0,
            bytemuck::cast_slice(&[EnvironmentUniform {
                sky_light: [sky_light_intensity, 0.0, 0.0, 0.0],
                ambient_light,
            }]),
        );
        let scene = self.scene_bind_group(
            device,
            camera_uniform_buffer,
            irradiance_map,
            prefilter_map,
            brdf_map,
        );
        let lights = {
            let scene_shader = self.scene_shader.read();
            let key = (
                scene_shader.bind_group_layouts[2].clone(),
                light_manager.storage_buffer(),
                shadows.data.clone(),
                [shadows.cascades.clone(), shadows.atlas.clone()],
                shadows.sampler.clone(),
            );
            self.light_storage_bind_group
                .get_or_create(key, || {
                    light_manager.storage_bind_group(device, &scene_shader, shadows)
                })
                .clone()
        };

        LightingBindGroups { scene, lights }
    }

    /// Draws `draws` of `draw_list` into `targets`, lit through `lighting`.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        assets: &LockedAssetRenderState,
        material_bind_groups: &MaterialBindGroups,
        lighting: &LightingBindGroups,
        targets: MeshRenderTargets<'_>,
        clear_color: Color32,
        pipeline_options: &PipelineOptions,
        draws: ForwardDraws,
        draw_list: &[DrawBatch],
    ) {
        let (color_load, depth_load) = match draws {
            ForwardDraws::All => (
                wgpu::LoadOp::Clear(RenderUtils::clear_value(clear_color)),
                wgpu::LoadOp::Clear(1.0),
            ),
            ForwardDraws::AfterDeferred { .. } => (wgpu::LoadOp::Load, wgpu::LoadOp::Load),
        };
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Viewport Scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &targets.color.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: color_load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &targets.depth.view,
                depth_ops: Some(wgpu::Operations {
                    load: depth_load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0),
                    store: wgpu::StoreOp::Store,
                }),
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        let mut last: (AssetId, AssetId, AssetId) = Default::default();
        for &DrawBatch {
            shader_id,
            mat_id,
            mesh_id,
            ref instances,
            ..
        } in draw_list
        {
            if !draws.includes(shader_id) {
                continue;
            }
            let Some(shader) = assets.shader(shader_id) else {
                continue;
            };
            let Some(mesh) = assets.mesh(mesh_id) else {
                continue;
            };
            if shader_id != last.0 {
                let Some(pipeline) = shader.pipeline(pipeline_options) else {
                    continue;
                };
                render_pass.set_pipeline(&pipeline);
                render_pass.set_bind_group(0, &lighting.scene, &[]);
                render_pass.set_bind_group(2, &lighting.lights, &[]);
            }
            if mat_id != last.1 {
                if let Some(groups) = material_bind_groups.get(&mat_id) {
                    for (index, group) in groups {
                        render_pass.set_bind_group(*index, group, &[]);
                    }
                }
            }
            if mesh_id != last.2 {
                let Some(mesh_instance_group) = mesh.instance_bind_group() else {
                    continue;
                };
                render_pass.set_bind_group(1, mesh_instance_group, &[]);
            }
            RenderUtils::bind_mesh_buffers(&mut render_pass, mesh);
            RenderUtils::draw_mesh_instanced(&mut render_pass, mesh, instances.clone());
            last = (shader_id, mat_id, mesh_id);
        }
    }

    fn scene_bind_group(
        &mut self,
        device: &wgpu::Device,
        camera_uniform_buffer: &wgpu::Buffer,
        irradiance_map: &Texture,
        prefilter_map: &Texture,
        brdf_map: &Texture,
    ) -> wgpu::BindGroup {
        let layout = self.scene_shader.read().bind_group_layouts[0].clone();
        let key = (
            layout.clone(),
            camera_uniform_buffer.clone(),
            [
                irradiance_map.view.clone(),
                prefilter_map.view.clone(),
                brdf_map.view.clone(),
            ],
            [
                irradiance_map.sampler.clone(),
                prefilter_map.sampler.clone(),
                brdf_map.sampler.clone(),
            ],
        );
        let environment_uniform_buffer = &self.environment_uniform_buffer;
        self.scene_bind_group
            .get_or_create(key, || {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("scene_bind_group"),
                    layout: &layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: camera_uniform_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&irradiance_map.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&irradiance_map.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(&prefilter_map.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: wgpu::BindingResource::Sampler(&prefilter_map.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: wgpu::BindingResource::TextureView(&brdf_map.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 6,
                            resource: wgpu::BindingResource::Sampler(&brdf_map.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 7,
                            resource: environment_uniform_buffer.as_entire_binding(),
                        },
                    ],
                })
            })
            .clone()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render_object_ids(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        assets: &LockedAssetRenderState,
        targets: MeshRenderTargets<'_>,
        camera_uniform_buffer: &wgpu::Buffer,
        draw_list: &[DrawBatch],
        gizmo_renderer: Option<&mut GizmoRenderer>,
    ) {
        let options = PipelineOptions::builder()
            .samples(1)
            .fragment_targets(vec![Some(wgpu::ColorTargetState {
                format: targets.color.descriptor.format,
                blend: None,
                write_mask: wgpu::ColorWrites::RED,
            })])
            .build();
        let shader = self.object_id_shader.read();
        let pipeline = shader.pipeline(&options);
        let layout = &shader.bind_group_layouts[0];
        let camera_bind_group = self
            .object_id_camera_bind_group
            .get_or_create((layout.clone(), camera_uniform_buffer.clone()), || {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("object_id_camera_bind_group"),
                    layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: camera_uniform_buffer.as_entire_binding(),
                    }],
                })
            })
            .clone();
        drop(shader);

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Viewport Object IDs"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &targets.color.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(RenderUtils::depth_stencil_attachment(
                &targets.depth.view,
                1.0,
                None,
            )),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        let Some(pipeline) = pipeline else {
            return;
        };
        render_pass.set_pipeline(&pipeline);
        render_pass.set_bind_group(0, &camera_bind_group, &[]);

        let mut last_mesh_id = AssetId::default();
        for &DrawBatch {
            mesh_id,
            ref instances,
            ..
        } in draw_list
        {
            let Some(mesh) = assets.mesh(mesh_id) else {
                continue;
            };
            if mesh_id != last_mesh_id {
                let Some(mesh_instance_group) = mesh.instance_bind_group() else {
                    continue;
                };
                render_pass.set_bind_group(1, mesh_instance_group, &[]);
                RenderUtils::bind_mesh_buffers(&mut render_pass, mesh);
                last_mesh_id = mesh_id;
            }
            RenderUtils::draw_mesh_instanced(&mut render_pass, mesh, instances.clone());
        }

        if let Some(gizmo_renderer) = gizmo_renderer {
            gizmo_renderer
                .render_icon_object_ids(targets.color.descriptor.format, &mut render_pass);
        }
    }

    /// Each drawn material's bind groups, by group index, reusing those built
    /// in earlier frames while the material binds the same resources. The
    /// scene pass and the masked shadow pass bind them.
    pub fn build_material_bind_groups(
        &mut self,
        device: &wgpu::Device,
        asset_context: &ReadOnlyAssetContext,
        default_texture: Ref<Texture>,
        assets: &LockedAssetRenderState,
    ) -> MaterialBindGroups {
        let live_materials: HashSet<AssetId> = assets.materials.keys().copied().collect();
        self.prune_material_bind_group_cache(&live_materials);

        let mut bind_groups: HashMap<AssetId, HashMap<u32, wgpu::BindGroup>> = Default::default();
        for (mat_id, mat) in assets.materials.iter() {
            let key = mat.bind_group_cache_key(asset_context, default_texture.clone());

            let groups = match self.material_bind_group_cache.get(mat_id) {
                Some(cached) if cached.key == key => cached.groups.clone(),
                _ => {
                    let groups =
                        mat.bind_groups(device, asset_context, assets, default_texture.clone());
                    self.material_bind_group_cache.insert(
                        *mat_id,
                        CachedMaterialBindGroups {
                            key,
                            groups: groups.clone(),
                        },
                    );
                    groups
                }
            };

            bind_groups.insert(*mat_id, groups);
        }
        bind_groups
    }

    fn prune_material_bind_group_cache(&mut self, live_materials: &HashSet<AssetId>) {
        self.material_bind_group_cache
            .retain(|mat_id, _| live_materials.contains(mat_id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::material::Material;
    use crate::assets::Asset;
    use crate::render::AssetRenderState;
    use crate::test_support::TestAssets;
    use uuid::Uuid;

    #[test]
    fn prunes_material_bind_group_cache_to_live_materials() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let asset_registry = read_only_context.registries.assets.read();
        let shader = asset_registry
            .load::<Shader>("shaders/mesh/pbr")
            .expect("missing pbr shader");
        let default_texture = asset_registry
            .white_texture()
            .expect("missing default material texture");
        drop(asset_registry);

        let material = Material::from_shader(&read_only_context, shader);
        let key = material.bind_group_cache_key(&read_only_context, default_texture);
        let live_id = Uuid::new_v4();
        let stale_id = Uuid::new_v4();
        let mut renderer =
            MeshRenderer::new(&read_only_context).expect("mesh renderer should load");
        renderer.material_bind_group_cache.insert(
            live_id,
            CachedMaterialBindGroups {
                key: key.clone(),
                groups: Default::default(),
            },
        );
        renderer.material_bind_group_cache.insert(
            stale_id,
            CachedMaterialBindGroups {
                key,
                groups: Default::default(),
            },
        );

        renderer.prune_material_bind_group_cache(&HashSet::from([live_id]));

        assert!(renderer.material_bind_group_cache.contains_key(&live_id));
        assert!(!renderer.material_bind_group_cache.contains_key(&stale_id));
    }

    /// The bind groups `renderer` builds for `material` this frame.
    fn material_bind_groups(
        renderer: &mut MeshRenderer,
        context: &ReadOnlyAssetContext,
        material: &Ref<Material>,
        default_texture: &Ref<Texture>,
    ) -> HashMap<u32, wgpu::BindGroup> {
        let mut state = AssetRenderState::default();
        state.materials.insert(material.id(), material.clone());
        let shader = material.read().shader.get_ref(&context.registries);
        let shader = shader.expect("the material's shader is loaded");
        state.shaders.insert(shader.id(), shader);
        material
            .read()
            .collect_textures(context, &mut state.textures, default_texture.clone());
        let locked = state.lock();
        let mut groups = renderer.build_material_bind_groups(
            context.render_context.device(),
            context,
            default_texture.clone(),
            &locked,
        );
        groups
            .remove(&material.id())
            .expect("the material has bind groups")
    }

    #[test]
    fn material_bind_groups_are_rebuilt_only_when_the_material_buffers_are_replaced() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let asset_registry = read_only_context.registries.assets.read();
        let shader = asset_registry
            .load::<Shader>("shaders/mesh/pbr")
            .expect("missing pbr shader");
        let default_texture = asset_registry
            .white_texture()
            .expect("missing default material texture");
        drop(asset_registry);
        let material = Ref::new(Material::from_shader(&read_only_context, shader.clone()));
        assert!(
            !material.read().buffers.is_empty(),
            "the pbr material binds a uniform buffer"
        );
        let mut renderer =
            MeshRenderer::new(&read_only_context).expect("mesh renderer should load");

        let first = material_bind_groups(
            &mut renderer,
            &read_only_context,
            &material,
            &default_texture,
        );
        assert!(!first.is_empty(), "the material binds at least one group");
        let unchanged = material_bind_groups(
            &mut renderer,
            &read_only_context,
            &material,
            &default_texture,
        );
        assert_eq!(
            unchanged, first,
            "an unchanged material reuses its cached bind groups"
        );

        // A hot reload replaces the material in place under the same id: the
        // same shader, so buffers of the same sizes, but new buffers.
        *material.write() = Material::from_shader(&read_only_context, shader);
        let reloaded = material_bind_groups(
            &mut renderer,
            &read_only_context,
            &material,
            &default_texture,
        );
        for (group, bind_group) in &first {
            assert_ne!(
                reloaded.get(group),
                Some(bind_group),
                "group {group} still binds the buffers the reload replaced"
            );
        }
    }

    #[test]
    fn object_id_shader_builds_pipeline_with_skinning_path() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let read_only_context = context.lock_read();
        let shader = Shader::from_file(
            &read_only_context,
            &assets.engine_root().join("shaders/mesh/object_id.wgsl"),
        )
        .expect("object id shader should load")
        .asset;
        let options = PipelineOptions::builder()
            .samples(1)
            .fragment_targets(vec![Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::R32Uint,
                blend: None,
                write_mask: wgpu::ColorWrites::RED,
            })])
            .build();

        let pipeline = shader.pipeline(&options);

        assert_eq!(shader.pipeline(&options), pipeline);
        assert_eq!(shader.pipeline_count(), 1);
    }
}
