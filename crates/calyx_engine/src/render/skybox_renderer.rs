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
    /// Whether the selected skybox is drawn as the sky, or only lights.
    visible: bool,
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
            visible: true,
            skybox_shader: load("shaders/environment/skybox")?,
            skybox_cubemap_shader: load("shaders/environment/cubemap")?,
            skybox_irradiance_cubemap_shader: load("shaders/environment/irradiance")?,
            skybox_prefilter_cubemap_shader: load("shaders/environment/prefilter")?,
            skybox_brdf_shader: load("shaders/environment/brdf")?,
            skybox_cubemap_mip_shader: load("shaders/utility/mip_generator_cube")?,
            cubemap_bind_group: Default::default(),
        })
    }

    pub fn skybox_id(&self) -> Option<AssetId> {
        self.skybox
    }

    pub fn set_skybox(&mut self, skybox: Option<Uuid>) {
        self.skybox = skybox;
    }

    /// Sets whether [`Self::render`] draws the selected skybox. A hidden
    /// skybox still lights the scene.
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    fn selected_skybox_ref(
        skybox: Option<Uuid>,
        assets: &AssetRenderState,
    ) -> Option<&Ref<Skybox>> {
        skybox.and_then(|id| assets.skybox(id))
    }

    /// Records the selected skybox's environment-map passes into `encoder`
    /// the first time it is used. Call it before anything in the frame draws
    /// with the skybox, so its maps are computed before they are read.
    pub fn prepare(
        &self,
        render_context: &RenderContext,
        encoder: &mut wgpu::CommandEncoder,
        assets: &AssetRenderState,
    ) {
        if let Some(skybox_ref) = Self::selected_skybox_ref(self.skybox, assets) {
            skybox_ref.write().prepare(
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
        }
    }

    /// Draws the selected skybox once [`Self::prepare`] has computed its maps.
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
        let skybox = Self::selected_skybox_ref(self.skybox, assets)
            .filter(|_| self.visible)
            .map(|skybox_ref| skybox_ref.read())
            .filter(|skybox| skybox.environment_maps().is_some());
        if let Some(skybox) = skybox {
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

    #[test]
    fn selected_skybox_maps_are_withheld_until_prepared() {
        let test_assets = crate::test_support::TestAssets::engine();
        let context = test_assets.asset_context();
        let context = context.lock_read();
        let render_context = context.render_context.clone();
        let mut renderer = SkyboxRenderer::new(&context).expect("skybox shaders should load");
        let source = Texture::solid_color_2d(render_context.clone(), "sky", [64, 128, 255, 255]);
        let skybox = Ref::new(Skybox::new(&render_context, source));
        let skybox_id = Uuid::new_v4();
        let mut assets = AssetRenderState::default();
        assets.skyboxes.insert(skybox_id, skybox.clone());
        renderer.set_skybox(Some(skybox_id));

        assert!(
            skybox.read().environment_maps().is_none(),
            "a skybox must not light or draw anything before its maps are computed"
        );

        let mut encoder =
            render_context
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("skybox_prepare_test"),
                });
        renderer.prepare(&render_context, &mut encoder, &assets);
        render_context.queue().submit([encoder.finish()]);

        let skybox = skybox.read();
        let (irradiance, _, _) = skybox
            .environment_maps()
            .expect("prepare should make the skybox's maps available");
        assert!(
            irradiance.descriptor.size.width <= 64,
            "the irradiance map should stay small enough to compute within a frame, got {}",
            irradiance.descriptor.size.width
        );
    }
}
