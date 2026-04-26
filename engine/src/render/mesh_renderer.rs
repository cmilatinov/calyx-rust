use crate::assets::material::MaterialBindGroupCacheKey;
use crate::assets::texture::Texture;
use crate::assets::AssetId;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::asset_render_state::LockedAssetRenderState;
use crate::render::render_utils::RenderUtils;
use crate::render::{GizmoRenderer, LightManager, PipelineOptions, Shader};
use egui::Color32;
use egui_wgpu::wgpu;
use std::collections::{HashMap, HashSet};
use std::ops::Range;

pub struct MeshRenderer {
    scene_shader: Ref<Shader>,
    material_bind_group_cache: HashMap<AssetId, CachedMaterialBindGroups>,
}

struct CachedMaterialBindGroups {
    key: MaterialBindGroupCacheKey,
    groups: HashMap<u32, wgpu::BindGroup>,
}

pub struct MeshRenderTargets<'a> {
    pub color: &'a Texture,
    pub depth: &'a Texture,
}

pub struct MeshRenderDefaults<'a> {
    pub missing_texture: Ref<Texture>,
    pub black_texture_2d: &'a Texture,
    pub black_texture_cube: &'a Texture,
}

impl MeshRenderer {
    pub fn new(context: &ReadOnlyAssetContext) -> Self {
        Self {
            scene_shader: context
                .registries
                .assets
                .read()
                .load::<Shader>("shaders/pbr")
                .expect("missing scene_shader"),
            material_bind_group_cache: Default::default(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        asset_context: &ReadOnlyAssetContext,
        assets: &LockedAssetRenderState,
        defaults: MeshRenderDefaults<'_>,
        targets: MeshRenderTargets<'_>,
        light_manager: &LightManager,
        camera_uniform_buffer: &wgpu::Buffer,
        clear_color: Color32,
        pipeline_options: &PipelineOptions,
        skybox_id: Option<AssetId>,
        draw_list: Vec<(AssetId, AssetId, AssetId, Range<u32>)>,
        gizmo_renderer: Option<&mut GizmoRenderer>,
    ) {
        let material_bind_groups = self.build_material_bind_groups(
            device,
            asset_context,
            defaults.missing_texture,
            assets,
        );
        let (irradiance_map, prefilter_map, brdf_map) = skybox_id
            .and_then(|id| {
                let skybox = assets.skybox(id)?;
                Some((
                    &skybox.irradiance_cubemap,
                    &skybox.prefilter_cubemap,
                    &skybox.brdf_map,
                ))
            })
            .unwrap_or((
                defaults.black_texture_cube,
                defaults.black_texture_cube,
                defaults.black_texture_2d,
            ));
        let scene_bind_group = self.scene_bind_group(
            device,
            camera_uniform_buffer,
            irradiance_map,
            prefilter_map,
            brdf_map,
        );
        let light_storage_bind_group = {
            let scene_shader = self.scene_shader.read();
            light_manager.storage_bind_group(device, &scene_shader)
        };

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Viewport Scene"),
            color_attachments: &[Some(RenderUtils::color_attachment(
                &targets.color.view,
                clear_color,
            ))],
            depth_stencil_attachment: Some(RenderUtils::depth_stencil_attachment(
                &targets.depth.view,
                1.0,
                Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0),
                    store: wgpu::StoreOp::Store,
                }),
            )),
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        let mut last: (AssetId, AssetId, AssetId) = Default::default();
        for (shader_id, mat_id, mesh_id, instances) in draw_list {
            let Some(shader) = assets.shader(shader_id) else {
                continue;
            };
            let Some(mesh) = assets.mesh(mesh_id) else {
                continue;
            };
            if shader_id != last.0 {
                if let Some(pipeline) = shader.get_pipeline(pipeline_options) {
                    render_pass.set_pipeline(pipeline);
                    render_pass.set_bind_group(0, &scene_bind_group, &[]);
                    render_pass.set_bind_group(2, &light_storage_bind_group, &[]);
                }
            }
            if mat_id != last.1 {
                if let Some(groups) = material_bind_groups.get(&mat_id) {
                    for (index, group) in groups {
                        render_pass.set_bind_group(*index, group, &[]);
                    }
                }
            }
            if mesh_id != last.2 {
                let Some(mesh_instance_group) = assets.mesh_instance_group(mesh_id) else {
                    continue;
                };
                render_pass.set_bind_group(1, mesh_instance_group, &[]);
            }
            RenderUtils::bind_mesh_buffers(&mut render_pass, mesh);
            RenderUtils::draw_mesh_instanced(&mut render_pass, mesh, instances);
            last = (shader_id, mat_id, mesh_id);
        }

        if let Some(gizmo_renderer) = gizmo_renderer {
            gizmo_renderer.render_gizmos(targets.color.descriptor.format, &mut render_pass);
        }
    }

    fn scene_bind_group(
        &self,
        device: &wgpu::Device,
        camera_uniform_buffer: &wgpu::Buffer,
        irradiance_map: &Texture,
        prefilter_map: &Texture,
        brdf_map: &Texture,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene_bind_group"),
            layout: &self.scene_shader.read().bind_group_layouts[0],
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
            ],
        })
    }

    fn build_material_bind_groups(
        &mut self,
        device: &wgpu::Device,
        asset_context: &ReadOnlyAssetContext,
        default_texture: Ref<Texture>,
        assets: &LockedAssetRenderState,
    ) -> HashMap<AssetId, HashMap<u32, wgpu::BindGroup>> {
        let live_materials: HashSet<AssetId> = assets.materials.keys().copied().collect();
        self.material_bind_group_cache
            .retain(|mat_id, _| live_materials.contains(mat_id));

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
}
