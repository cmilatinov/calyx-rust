use crate::assets::mesh::Mesh;
use crate::assets::skybox::Skybox;
use crate::assets::skybox::SkyboxShaders;
use crate::assets::texture::Texture;
use crate::assets::AssetId;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::asset_render_state::AssetRenderState;
use crate::render::render_utils::RenderUtils;
use crate::render::{PipelineOptions, Shader};
use egui_wgpu::{wgpu, RenderState};
use uuid::Uuid;

pub struct SkyboxRenderer {
    skybox: Option<Uuid>,
    skybox_shader: Ref<Shader>,
    skybox_cubemap_shader: Ref<Shader>,
    skybox_irradiance_cubemap_shader: Ref<Shader>,
    skybox_prefilter_cubemap_shader: Ref<Shader>,
    skybox_brdf_shader: Ref<Shader>,
    skybox_cubemap_mip_shader: Ref<Shader>,
}

impl SkyboxRenderer {
    pub fn new(context: &ReadOnlyAssetContext) -> Self {
        let asset_registry = context.registries.assets.read();
        Self {
            skybox: None,
            skybox_shader: asset_registry
                .load::<Shader>("shaders/environment/skybox")
                .expect("missing skybox_shader"),
            skybox_cubemap_shader: asset_registry
                .load::<Shader>("shaders/environment/cubemap")
                .expect("missing skybox_cubemap_shader"),
            skybox_irradiance_cubemap_shader: asset_registry
                .load::<Shader>("shaders/environment/irradiance")
                .expect("missing skybox_irradiance_cubemap_shader"),
            skybox_prefilter_cubemap_shader: asset_registry
                .load::<Shader>("shaders/environment/prefilter")
                .expect("missing skybox_prefilter_cubemap_shader"),
            skybox_brdf_shader: asset_registry
                .load::<Shader>("shaders/environment/brdf")
                .expect("missing skybox_brdf_shader"),
            skybox_cubemap_mip_shader: asset_registry
                .load::<Shader>("shaders/mip_generator_cube")
                .expect("missing skybox_cubemap_mip_shader"),
        }
    }

    pub fn skybox_id(&self) -> Option<AssetId> {
        self.skybox
    }

    pub fn set_skybox(&mut self, skybox: Option<Uuid>) {
        self.skybox = skybox;
    }

    fn selected_skybox_ref(
        skybox: Option<Uuid>,
        assets: &AssetRenderState,
    ) -> Option<&Ref<Skybox>> {
        skybox.and_then(|id| assets.skybox(id))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        render_state: &RenderState,
        encoder: &mut wgpu::CommandEncoder,
        assets: &AssetRenderState,
        cube_mesh: &Ref<Mesh>,
        scene_texture_msaa: &Texture,
        scene_depth_texture: &Texture,
        camera_bind_group: &wgpu::BindGroup,
        samples: u32,
    ) {
        if let Some(skybox_ref) = Self::selected_skybox_ref(self.skybox, assets) {
            let mut skybox = skybox_ref.write();
            skybox.prepare(
                SkyboxShaders {
                    cubemap_shader: &self.skybox_cubemap_shader,
                    irradiance_cubemap_shader: &self.skybox_irradiance_cubemap_shader,
                    prefilter_cubemap_shader: &self.skybox_prefilter_cubemap_shader,
                    brdf_shader: &self.skybox_brdf_shader,
                    cubemap_mip_shader: &self.skybox_cubemap_mip_shader,
                },
                render_state,
                encoder,
            );

            let device = &render_state.device;
            let mut shader = self.skybox_shader.write();
            let cube_mesh = cube_mesh.read();
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &shader.bind_group_layouts[1],
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&skybox.cubemap.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&skybox.cubemap.sampler),
                    },
                ],
            });
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Skybox"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &scene_texture_msaa.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &scene_depth_texture.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let options = PipelineOptions::builder()
                .fragment_targets(vec![Some(wgpu::ColorTargetState {
                    format: scene_texture_msaa.descriptor.format,
                    blend: None,
                    write_mask: Default::default(),
                })])
                .samples(samples)
                .cull_mode(Some(wgpu::Face::Front))
                .build();
            shader.build_pipeline(&options);
            if let Some(pipeline) = shader.get_pipeline(&options) {
                render_pass.set_pipeline(pipeline);
                render_pass.set_bind_group(0, camera_bind_group, &[]);
                render_pass.set_bind_group(1, &bind_group, &[]);
                RenderUtils::bind_mesh_buffers(&mut render_pass, &cube_mesh);
                RenderUtils::draw_mesh_instanced(&mut render_pass, &cube_mesh, 0..1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_skybox_ref_returns_none_without_selection() {
        let assets = AssetRenderState::default();

        assert!(SkyboxRenderer::selected_skybox_ref(None, &assets).is_none());
    }

    #[test]
    fn selected_skybox_ref_returns_none_for_missing_asset() {
        let assets = AssetRenderState::default();
        let skybox_id = Uuid::new_v4();

        assert!(SkyboxRenderer::selected_skybox_ref(Some(skybox_id), &assets).is_none());
    }
}
