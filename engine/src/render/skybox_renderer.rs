use crate::assets::mesh::Mesh;
use crate::assets::skybox::Skybox;
use crate::assets::skybox::SkyboxShaders;
use crate::assets::texture::Texture;
use crate::assets::AssetId;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::asset_render_state::AssetRenderState;
use crate::render::render_utils::RenderUtils;
use crate::render::{CachedBindGroup, PipelineOptions, RenderContext, SceneRendererError, Shader};
use egui_wgpu::wgpu;
use uuid::Uuid;

pub struct SkyboxRenderer {
    skybox: Option<Uuid>,
    skybox_shader: Ref<Shader>,
    skybox_cubemap_shader: Ref<Shader>,
    skybox_irradiance_cubemap_shader: Ref<Shader>,
    skybox_prefilter_cubemap_shader: Ref<Shader>,
    skybox_brdf_shader: Ref<Shader>,
    skybox_cubemap_mip_shader: Ref<Shader>,
    cubemap_bind_group: CachedBindGroup<(wgpu::BindGroupLayout, wgpu::TextureView, wgpu::Sampler)>,
}

impl SkyboxRenderer {
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        let load = |name| SceneRendererError::load::<Shader>(context, name);
        Ok(Self {
            skybox: None,
            skybox_shader: load("shaders/environment/skybox")?,
            skybox_cubemap_shader: load("shaders/environment/cubemap")?,
            skybox_irradiance_cubemap_shader: load("shaders/environment/irradiance")?,
            skybox_prefilter_cubemap_shader: load("shaders/environment/prefilter")?,
            skybox_brdf_shader: load("shaders/environment/brdf")?,
            skybox_cubemap_mip_shader: load("shaders/mip_generator_cube")?,
            cubemap_bind_group: Default::default(),
        })
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
        render_context: &RenderContext,
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
                render_context,
                encoder,
            );

            let device = render_context.device();
            let shader = self.skybox_shader.read();
            let cube_mesh = cube_mesh.read();
            let layout = &shader.bind_group_layouts[1];
            let cubemap = &skybox.cubemap;
            let bind_group = self.cubemap_bind_group.get_or_create(
                (
                    layout.clone(),
                    cubemap.view.clone(),
                    cubemap.sampler.clone(),
                ),
                || {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("skybox_cubemap_bind_group"),
                        layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&cubemap.view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(&cubemap.sampler),
                            },
                        ],
                    })
                },
            );
            let mut render_pass = RenderUtils::overlay_pass(
                encoder,
                &scene_texture_msaa.view,
                Some(&scene_depth_texture.view),
                "Skybox",
            );
            let options = PipelineOptions::builder()
                .fragment_targets(vec![Some(wgpu::ColorTargetState {
                    format: scene_texture_msaa.descriptor.format,
                    blend: None,
                    write_mask: Default::default(),
                })])
                .samples(samples)
                .cull_mode(Some(wgpu::Face::Front))
                .build();
            render_pass.set_pipeline(&shader.pipeline(&options));
            render_pass.set_bind_group(0, camera_bind_group, &[]);
            render_pass.set_bind_group(1, bind_group, &[]);
            RenderUtils::bind_mesh_buffers(&mut render_pass, &cube_mesh);
            RenderUtils::draw_mesh_instanced(&mut render_pass, &cube_mesh, 0..1);
        } else {
            // Release the cubemap of a skybox no longer drawn.
            self.cubemap_bind_group.clear();
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
