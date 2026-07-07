use super::cache::*;
use super::camera_fit::*;
use super::*;

pub(super) struct ThumbnailGenerator {
    pub(super) render_settings: ThumbnailRenderSettings,
    texture_downscaler: Option<TextureDownscaler>,
    scene_renderer: Option<SceneRenderer>,
    skybox_front_face_downscaler: Option<SkyboxFrontFaceDownscaler>,
}

impl Default for ThumbnailGenerator {
    fn default() -> Self {
        Self {
            render_settings: ThumbnailRenderSettings::default(),
            texture_downscaler: None,
            scene_renderer: None,
            skybox_front_face_downscaler: None,
        }
    }
}
impl ThumbnailGenerator {
    pub(super) fn with_render_settings(render_settings: ThumbnailRenderSettings) -> Self {
        Self {
            render_settings,
            ..Default::default()
        }
    }

    pub(super) fn generate(
        &mut self,
        context: &ReadOnlyAssetContext,
        render_state: &RenderState,
        request: &ThumbnailRequest,
    ) -> Result<Texture, String> {
        if request.asset_type == Texture::type_uuid() {
            return self.generate_texture_thumbnail(context, render_state, request.asset_id);
        }
        if request.asset_type == Material::type_uuid() {
            return self.generate_material_thumbnail(context, render_state, request.asset_id);
        }
        if request.asset_type == Mesh::type_uuid() {
            return self.generate_mesh_thumbnail(context, render_state, request.asset_id);
        }
        if request.asset_type == Prefab::type_uuid() {
            return self.generate_prefab_thumbnail(context, render_state, request.asset_id);
        }
        if request.asset_type == Skybox::type_uuid() {
            return self.generate_skybox_thumbnail(context, render_state, request);
        }
        Err(format!(
            "unsupported thumbnail asset type {}",
            request.asset_type
        ))
    }

    fn generate_texture_thumbnail(
        &mut self,
        context: &ReadOnlyAssetContext,
        render_state: &RenderState,
        asset_id: Uuid,
    ) -> Result<Texture, String> {
        let texture_ref = context
            .registries
            .assets
            .read()
            .load_by_id::<Texture>(asset_id)
            .map_err(|err| format!("failed to load texture thumbnail source: {err}"))?;
        let texture = texture_ref.read();
        if texture.descriptor.dimension != wgpu::TextureDimension::D2
            || texture.descriptor.size.depth_or_array_layers != 1
        {
            return Err("texture thumbnails require a 2D source texture".into());
        }
        if self.texture_downscaler.is_none() {
            self.texture_downscaler = Some(TextureDownscaler::new(context)?);
        }
        let Some(downscaler) = &self.texture_downscaler else {
            return Err("texture thumbnail downscaler was not initialized".into());
        };
        Ok(downscaler.downscale(
            context,
            render_state,
            &texture,
            self.render_settings.size_px,
        ))
    }

    fn generate_material_thumbnail(
        &mut self,
        context: &ReadOnlyAssetContext,
        render_state: &RenderState,
        asset_id: Uuid,
    ) -> Result<Texture, String> {
        let material_ref = context
            .registries
            .assets
            .read()
            .load_by_id::<Material>(asset_id)
            .map_err(|err| format!("failed to load material thumbnail source: {err}"))?;
        let sphere_ref = context
            .registries
            .assets
            .read()
            .sphere()
            .ok_or_else(|| "missing sphere mesh for material thumbnail render".to_string())?;
        let camera_fit = {
            let sphere = sphere_ref.read();
            CameraFit::from_mesh(&sphere, &Transform::default()).unwrap_or_default()
        };
        let mut scene = context.scene();
        let game_object = scene.create(None, None);
        scene.add_component(
            game_object,
            ComponentMesh {
                mesh: Some(sphere_ref).into(),
                material: Some(material_ref).into(),
            },
        );
        add_preview_lighting(&mut scene);
        self.render_scene_thumbnail(
            context,
            render_state,
            &scene,
            camera_fit,
            self.render_settings.frame_margin,
        )
    }

    fn generate_mesh_thumbnail(
        &mut self,
        context: &ReadOnlyAssetContext,
        render_state: &RenderState,
        asset_id: Uuid,
    ) -> Result<Texture, String> {
        let mesh_ref = context
            .registries
            .assets
            .read()
            .load_by_id::<Mesh>(asset_id)
            .map_err(|err| format!("failed to load mesh thumbnail source: {err}"))?;
        let material = default_material_ref(context)?;
        let camera_fit = {
            let mesh = mesh_ref.read();
            CameraFit::from_mesh(&mesh, &Transform::default()).unwrap_or_default()
        };
        let mut scene = context.scene();
        let game_object = scene.create(None, None);
        scene.add_component(
            game_object,
            ComponentMesh {
                mesh: AssetRef::from_id(asset_id),
                material,
            },
        );
        add_preview_lighting(&mut scene);
        self.render_scene_thumbnail(
            context,
            render_state,
            &scene,
            camera_fit,
            self.render_settings.frame_margin,
        )
    }

    fn generate_skybox_thumbnail(
        &mut self,
        context: &ReadOnlyAssetContext,
        render_state: &RenderState,
        request: &ThumbnailRequest,
    ) -> Result<Texture, String> {
        let source_path = request
            .source_path
            .as_deref()
            .ok_or_else(|| "skybox thumbnail source has no file path".to_string())?;
        let source = texture_from_image_file(context, "thumbnail_skybox_source", source_path)
            .map_err(|err| format!("failed to load skybox thumbnail source: {err}"))?;
        if source.descriptor.dimension != wgpu::TextureDimension::D2
            || source.descriptor.size.depth_or_array_layers != 1
        {
            return Err("skybox thumbnails require a 2D source texture".into());
        }
        if self.skybox_front_face_downscaler.is_none() {
            self.skybox_front_face_downscaler = Some(SkyboxFrontFaceDownscaler::new(context)?);
        }
        let Some(downscaler) = &self.skybox_front_face_downscaler else {
            return Err("skybox thumbnail downscaler was not initialized".into());
        };
        Ok(downscaler.downscale_front_face(
            context,
            render_state,
            &source,
            self.render_settings.size_px,
        ))
    }

    fn generate_prefab_thumbnail(
        &mut self,
        context: &ReadOnlyAssetContext,
        render_state: &RenderState,
        asset_id: Uuid,
    ) -> Result<Texture, String> {
        let prefab_ref = context
            .registries
            .assets
            .read()
            .load_by_id::<Prefab>(asset_id)
            .map_err(|err| format!("failed to load prefab thumbnail source: {err}"))?;
        let mut scene = context.scene();
        let root = {
            let prefab = prefab_ref.read();
            scene
                .instantiate_prefab(&prefab, None)
                .ok_or_else(|| "prefab thumbnail source could not be instantiated".to_string())?
        };
        let camera_fit = scene_mesh_camera_fit(context, &scene, root).unwrap_or_default();
        add_preview_lighting(&mut scene);
        self.render_scene_thumbnail(
            context,
            render_state,
            &scene,
            camera_fit,
            self.render_settings.frame_margin,
        )
    }

    fn render_scene_thumbnail(
        &mut self,
        context: &ReadOnlyAssetContext,
        render_state: &RenderState,
        scene: &Scene,
        camera_fit: CameraFit,
        frame_margin: f32,
    ) -> Result<Texture, String> {
        let size_px = self.render_settings.size_px;
        let (camera, camera_transform) = camera_for_fit(&camera_fit, frame_margin);
        {
            let renderer = self.scene_renderer.get_or_insert_with(|| {
                SceneRenderer::new(
                    context,
                    SceneRendererOptions {
                        grid: false,
                        gizmos: false,
                        samples: 1,
                        clear_color: Color32::TRANSPARENT,
                        mesh_cull_mode: None,
                    },
                    (size_px, size_px),
                )
            });
            renderer.resize_textures(size_px, size_px);
            renderer.render_scene_base(render_state, &camera, &camera_transform, scene, None);
            renderer.finalize_scene(render_state);
        }

        if self.texture_downscaler.is_none() {
            self.texture_downscaler = Some(TextureDownscaler::new(context)?);
        }
        let Some(downscaler) = &self.texture_downscaler else {
            return Err("texture thumbnail downscaler was not initialized".into());
        };
        let Some(renderer) = &self.scene_renderer else {
            return Err("thumbnail scene renderer was not initialized".into());
        };
        Ok(downscaler.downscale(context, render_state, renderer.scene_texture(), size_px))
    }
}

pub(super) struct SkyboxFrontFaceDownscaler {
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

impl SkyboxFrontFaceDownscaler {
    pub(super) fn new(context: &ReadOnlyAssetContext) -> Result<Self, String> {
        let shader_ref = context
            .registries
            .assets
            .read()
            .load::<Shader>("shaders/thumbnail_skybox_front_face")
            .map_err(|err| format!("failed to load skybox thumbnail shader: {err}"))?;
        let shader = shader_ref.read();
        let pipeline = shader.compute_pipeline.as_ref().cloned().ok_or_else(|| {
            "skybox thumbnail shader did not build a compute pipeline".to_string()
        })?;
        let bind_group_layout =
            shader.bind_group_layouts.first().cloned().ok_or_else(|| {
                "skybox thumbnail shader did not declare bind group 0".to_string()
            })?;
        Ok(Self {
            pipeline,
            bind_group_layout,
        })
    }

    fn downscale_front_face(
        &self,
        context: &ReadOnlyAssetContext,
        render_state: &RenderState,
        source: &Texture,
        size_px: u32,
    ) -> Texture {
        let thumbnail =
            thumbnail_output_texture(context, "thumbnail_skybox_front_face_output", size_px);
        let thumbnail_storage_view = thumbnail_storage_view(&thumbnail);
        let bind_group = render_state
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("thumbnail_skybox_front_face_bind_group"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&source.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&thumbnail_storage_view),
                    },
                ],
            });
        let mut encoder =
            render_state
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("thumbnail_skybox_front_face_encoder"),
                });
        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("thumbnail_skybox_front_face_pass"),
                timestamp_writes: None,
            });
            compute_pass.set_pipeline(&self.pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);
            compute_pass.dispatch_workgroups(size_px.div_ceil(8), size_px.div_ceil(8), 1);
        }
        render_state.queue.submit(Some(encoder.finish()));
        thumbnail
    }
}

struct TextureDownscaler {
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

fn thumbnail_output_texture(
    context: &ReadOnlyAssetContext,
    label: &'static str,
    size_px: u32,
) -> Texture {
    Texture::new(
        context.render_context.clone(),
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size_px,
                height: size_px,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
        },
        None,
        Some(thumbnail_display_view_descriptor()),
        true,
    )
}

impl TextureDownscaler {
    fn new(context: &ReadOnlyAssetContext) -> Result<Self, String> {
        let shader_ref = context
            .registries
            .assets
            .read()
            .load::<Shader>("shaders/thumbnail_downscale")
            .map_err(|err| format!("failed to load thumbnail downscale shader: {err}"))?;
        let shader = shader_ref.read();
        let pipeline = shader.compute_pipeline.as_ref().cloned().ok_or_else(|| {
            "thumbnail downscale shader did not build a compute pipeline".to_string()
        })?;
        let bind_group_layout =
            shader.bind_group_layouts.first().cloned().ok_or_else(|| {
                "thumbnail downscale shader did not declare bind group 0".to_string()
            })?;
        Ok(Self {
            pipeline,
            bind_group_layout,
        })
    }

    fn downscale(
        &self,
        context: &ReadOnlyAssetContext,
        render_state: &RenderState,
        source: &Texture,
        size_px: u32,
    ) -> Texture {
        self.downscale_view(
            context,
            render_state,
            &source.view,
            &source.sampler,
            size_px,
        )
    }

    fn downscale_view(
        &self,
        context: &ReadOnlyAssetContext,
        render_state: &RenderState,
        source_view: &wgpu::TextureView,
        source_sampler: &wgpu::Sampler,
        size_px: u32,
    ) -> Texture {
        let thumbnail =
            thumbnail_output_texture(context, "thumbnail_texture_downscale_output", size_px);
        let thumbnail_storage_view = thumbnail_storage_view(&thumbnail);
        let bind_group = render_state
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("thumbnail_downscale_bind_group"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(source_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(source_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&thumbnail_storage_view),
                    },
                ],
            });
        let mut encoder =
            render_state
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("thumbnail_downscale_encoder"),
                });
        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("thumbnail_downscale_pass"),
                timestamp_writes: None,
            });
            compute_pass.set_pipeline(&self.pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);
            compute_pass.dispatch_workgroups(size_px.div_ceil(8), size_px.div_ceil(8), 1);
        }
        render_state.queue.submit(Some(encoder.finish()));
        thumbnail
    }
}
fn default_material_ref(context: &ReadOnlyAssetContext) -> Result<AssetRef<Material>, String> {
    context
        .registries
        .assets
        .read()
        .asset_id("materials/default")
        .map(AssetRef::from_id)
        .ok_or_else(|| "missing default material for thumbnail render".into())
}

fn add_preview_lighting(scene: &mut Scene) {
    let ambient = scene.create(None, None);
    scene.add_component(
        ambient,
        ComponentAmbientLight {
            active: true,
            color: Color32::from_rgb(210, 220, 232),
            intensity: 0.12,
        },
    );

    let directional = scene.create(None, None);
    let mut directional_transform = Transform::from_xyz(-3.0, 4.5, -4.0);
    directional_transform.look_at(&vec3(0.0, 0.0, 0.0));
    scene.set_world_transform(directional, directional_transform.matrix());
    scene.add_component(
        directional,
        ComponentDirectionalLight {
            active: true,
            color: Color32::WHITE,
            intensity: 0.72,
        },
    );

    let point = scene.create(None, None);
    scene.set_world_transform(point, Transform::from_xyz(2.25, 2.5, -2.75).matrix());
    scene.add_component(
        point,
        ComponentPointLight {
            active: true,
            radius: 6.0,
            color: Color32::WHITE,
            intensity: 0.45,
        },
    );
}
