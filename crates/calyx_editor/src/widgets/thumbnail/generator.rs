use super::cache::*;
use super::camera_fit::*;
use super::*;

#[derive(Default)]
pub(super) struct ThumbnailGenerator {
    pub(super) render_settings: ThumbnailRenderSettings,
    texture_downscaler: Option<TextureDownscaler>,
    scene_renderer: Option<SceneRenderer>,
    skybox_front_face_downscaler: Option<SkyboxFrontFaceDownscaler>,
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
        gpu: &RenderContext,
        request: &ThumbnailRequest,
    ) -> Result<Texture, String> {
        if request.asset_type == Texture::type_uuid() {
            return self.generate_texture_thumbnail(context, gpu, request.asset_id);
        }
        if request.asset_type == Material::type_uuid() {
            return self.generate_material_thumbnail(context, gpu, request.asset_id);
        }
        if request.asset_type == Mesh::type_uuid() {
            return self.generate_mesh_thumbnail(context, gpu, request.asset_id);
        }
        if request.asset_type == Prefab::type_uuid() {
            return self.generate_prefab_thumbnail(context, gpu, request.asset_id);
        }
        if request.asset_type == Skybox::type_uuid() {
            return self.generate_skybox_thumbnail(context, gpu, request);
        }
        if request.asset_type == Scene::type_uuid() {
            return self.generate_scene_thumbnail(context, gpu, request);
        }
        Err(format!(
            "unsupported thumbnail asset type {}",
            request.asset_type
        ))
    }

    fn generate_texture_thumbnail(
        &mut self,
        context: &ReadOnlyAssetContext,
        gpu: &RenderContext,
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
        Ok(downscaler.downscale(context, gpu, &texture, self.render_settings.size_px))
    }

    fn generate_material_thumbnail(
        &mut self,
        context: &ReadOnlyAssetContext,
        gpu: &RenderContext,
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
                ..Default::default()
            },
        );
        add_preview_lighting(&mut scene);
        self.render_scene_thumbnail(
            context,
            gpu,
            &scene,
            camera_fit,
            self.render_settings.frame_margin,
        )
    }

    fn generate_mesh_thumbnail(
        &mut self,
        context: &ReadOnlyAssetContext,
        gpu: &RenderContext,
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
                ..Default::default()
            },
        );
        add_preview_lighting(&mut scene);
        self.render_scene_thumbnail(
            context,
            gpu,
            &scene,
            camera_fit,
            self.render_settings.frame_margin,
        )
    }

    fn generate_skybox_thumbnail(
        &mut self,
        context: &ReadOnlyAssetContext,
        gpu: &RenderContext,
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
        Ok(downscaler.downscale_front_face(context, gpu, &source, self.render_settings.size_px))
    }

    fn generate_prefab_thumbnail(
        &mut self,
        context: &ReadOnlyAssetContext,
        gpu: &RenderContext,
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
            scene.instantiate_prefab(&prefab, None).map_err(|err| {
                format!("prefab thumbnail source could not be instantiated: {err}")
            })?
        };
        let camera_fit = scene_mesh_camera_fit(context, &scene, root).unwrap_or_default();
        add_preview_lighting(&mut scene);
        self.render_scene_thumbnail(
            context,
            gpu,
            &scene,
            camera_fit,
            self.render_settings.frame_margin,
        )
    }

    /// Renders the scene at the request's `.cxscene` file as authored: loaded
    /// straight from the file, never through the registry's shared copy, so
    /// a scene open and edited in the editor is never replaced.
    fn generate_scene_thumbnail(
        &mut self,
        context: &ReadOnlyAssetContext,
        gpu: &RenderContext,
        request: &ThumbnailRequest,
    ) -> Result<Texture, String> {
        let source_path = request
            .source_path
            .as_deref()
            .ok_or_else(|| "scene thumbnail source has no file path".to_string())?;
        let started_at = Instant::now();
        let mut scene = Scene::from_file(context, source_path)
            .map_err(|err| format!("failed to load scene thumbnail source: {err}"))?
            .asset;
        load_scene_sky_lights(context, &scene);
        fill_missing_materials(context, &mut scene)?;
        let loaded_at = Instant::now();

        let view = scene_thumbnail_view(context, &scene, self.render_settings.frame_margin);
        if !scene_has_light(&scene) {
            add_preview_lighting(&mut scene);
        }
        let texture = self.render_scene_view(context, gpu, &scene, &view)?;
        log::debug!(
            "Rendered scene thumbnail path={} objects={} camera={} load_ms={} render_ms={}",
            source_path.display(),
            scene.objects().count(),
            if view.from_main_camera { "main" } else { "fit" },
            loaded_at.duration_since(started_at).as_millis(),
            loaded_at.elapsed().as_millis()
        );
        Ok(texture)
    }

    fn render_scene_thumbnail(
        &mut self,
        context: &ReadOnlyAssetContext,
        gpu: &RenderContext,
        scene: &Scene,
        camera_fit: CameraFit,
        frame_margin: f32,
    ) -> Result<Texture, String> {
        let view = SceneThumbnailView::fit(&camera_fit, frame_margin);
        self.render_scene_view(context, gpu, scene, &view)
    }

    fn render_scene_view(
        &mut self,
        context: &ReadOnlyAssetContext,
        gpu: &RenderContext,
        scene: &Scene,
        view: &SceneThumbnailView,
    ) -> Result<Texture, String> {
        let size_px = self.render_settings.size_px;
        {
            if self.scene_renderer.is_none() {
                self.scene_renderer = Some(
                    SceneRenderer::new(
                        context,
                        SceneRendererOptions {
                            grid: false,
                            gizmos: false,
                            samples: 1,
                            clear_color: Color32::TRANSPARENT,
                            mesh_cull_mode: None,
                            environment: None,
                            // Thumbnails skip the shadow pass and its maps.
                            shadows: false,
                            // Thumbnails show the lit image.
                            gbuffer_view: None,
                        },
                        (size_px, size_px),
                    )
                    .map_err(|error| error.to_string())?,
                );
            }
            let Some(renderer) = self.scene_renderer.as_mut() else {
                return Err("thumbnail scene renderer was not initialized".into());
            };
            renderer.options_mut().clear_color = view.clear_color;
            renderer.resize_textures(size_px, size_px);
            renderer.render_scene_base(&view.camera, &view.camera_transform, scene, None, None);
            renderer.finalize_scene();
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
        Ok(downscaler.downscale(context, gpu, renderer.scene_texture(), size_px))
    }
}

/// Where a scene thumbnail is rendered from and what shows behind it.
pub(super) struct SceneThumbnailView {
    pub(super) camera: Camera,
    pub(super) camera_transform: Transform,
    pub(super) clear_color: Color32,
    /// Whether the view is the scene's main camera rather than a bounds fit.
    pub(super) from_main_camera: bool,
}

impl SceneThumbnailView {
    fn fit(camera_fit: &CameraFit, frame_margin: f32) -> Self {
        let (camera, camera_transform) = camera_for_fit(camera_fit, frame_margin);
        Self {
            camera,
            camera_transform,
            clear_color: Color32::TRANSPARENT,
            from_main_camera: false,
        }
    }
}

/// The view a scene thumbnail renders from: the scene's enabled main camera,
/// with its world transform, field of view, planes and clear color at a
/// square aspect, else a fit of the bounds of the scene's meshes.
pub(super) fn scene_thumbnail_view(
    context: &ReadOnlyAssetContext,
    scene: &Scene,
    frame_margin: f32,
) -> SceneThumbnailView {
    if let Some((game_object, camera)) = scene.main_camera() {
        return SceneThumbnailView {
            camera: Camera::new(1.0, camera.fov, camera.near_plane, camera.far_plane),
            camera_transform: scene.world_transform(game_object),
            clear_color: camera.clear_color,
            from_main_camera: true,
        };
    }
    let camera_fit = scene_bounds_camera_fit(context, scene).unwrap_or_default();
    SceneThumbnailView::fit(&camera_fit, frame_margin)
}

/// Whether `scene` holds a light that lights it: an enabled ambient,
/// directional, point or spot light, or an enabled sky light with a
/// skybox.
pub(super) fn scene_has_light(scene: &Scene) -> bool {
    scene.objects().any(|game_object| {
        has_enabled::<ComponentAmbientLight>(scene, game_object)
            || has_enabled::<ComponentDirectionalLight>(scene, game_object)
            || has_enabled::<ComponentPointLight>(scene, game_object)
            || has_enabled::<ComponentSpotLight>(scene, game_object)
            || (scene.is_component_enabled::<ComponentSkyLight>(game_object)
                && scene
                    .read_component::<ComponentSkyLight, _, _>(game_object, |light| {
                        !light.skybox.id().is_nil()
                    })
                    .unwrap_or(false))
    })
}

/// Whether `game_object` has a component `T` the scene holds enabled.
fn has_enabled<T: Component>(scene: &Scene, game_object: GameObject) -> bool {
    scene
        .read_component::<T, _, _>(game_object, |_| ())
        .is_some()
        && scene.is_component_enabled::<T>(game_object)
}

/// Gives the default material to each mesh that has none. The scene renderer
/// skips a mesh without a material, so a scene authored that way, such as an
/// animation demo, would otherwise render an empty thumbnail. Only the
/// worker's own copy of the scene changes, never the file.
fn fill_missing_materials(context: &ReadOnlyAssetContext, scene: &mut Scene) -> Result<(), String> {
    let objects = scene.objects().collect::<Vec<_>>();
    let mut default_material = None;
    for game_object in objects {
        let missing_mesh_material = scene
            .read_component::<ComponentMesh, _, _>(game_object, |mesh| mesh.material.id().is_nil())
            .unwrap_or(false);
        let missing_skinned_material = scene
            .read_component::<ComponentSkinnedMesh, _, _>(game_object, |mesh| {
                mesh.material.id().is_nil()
            })
            .unwrap_or(false);
        if !missing_mesh_material && !missing_skinned_material {
            continue;
        }
        let material = match &default_material {
            Some(material) => AssetRef::<Material>::from_id(*material),
            None => {
                let material = default_material_ref(context)?;
                default_material = Some(material.id());
                material
            }
        };
        if missing_mesh_material {
            let material = AssetRef::from_id(material.id());
            scene.write_component::<ComponentMesh, _>(game_object, |mesh| {
                mesh.material = material;
            });
        }
        if missing_skinned_material {
            scene.write_component::<ComponentSkinnedMesh, _>(game_object, |mesh| {
                mesh.material = material;
            });
        }
    }
    Ok(())
}

/// Loads the skyboxes of the scene's sky lights now. The scene renderer
/// loads a skybox in the background and draws without it until it is ready,
/// which a single thumbnail render would never wait for.
fn load_scene_sky_lights(context: &ReadOnlyAssetContext, scene: &Scene) {
    let skybox_ids = scene
        .objects()
        .filter_map(|game_object| {
            scene
                .read_component::<ComponentSkyLight, _, _>(game_object, |light| light.skybox.id())
                .filter(|_| scene.is_component_enabled::<ComponentSkyLight>(game_object))
        })
        .filter(|id| !id.is_nil())
        .collect::<HashSet<_>>();
    for skybox_id in skybox_ids {
        if let Err(err) = context
            .registries
            .assets
            .read()
            .load_by_id::<Skybox>(skybox_id)
        {
            log::warn!("Scene thumbnail sky light skybox {skybox_id} failed to load: {err}");
        }
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
            .load::<Shader>("shaders/editor/thumbnail_skybox_front_face")
            .map_err(|err| format!("failed to load skybox thumbnail shader: {err}"))?;
        let shader = shader_ref.read();
        let pipeline = shader
            .compute_pipeline("compute_main")
            .map(|kernel| kernel.pipeline)
            .ok_or_else(|| {
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
        gpu: &RenderContext,
        source: &Texture,
        size_px: u32,
    ) -> Texture {
        let thumbnail =
            thumbnail_output_texture(context, "thumbnail_skybox_front_face_output", size_px);
        let thumbnail_storage_view = thumbnail_storage_view(&thumbnail);
        let bind_group = gpu.device().create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("thumbnail_skybox_front_face_bind_group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&source.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&source.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&thumbnail_storage_view),
                },
            ],
        });
        let mut encoder = gpu
            .device()
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
        gpu.queue().submit(Some(encoder.finish()));
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
            .load::<Shader>("shaders/editor/thumbnail_downscale")
            .map_err(|err| format!("failed to load thumbnail downscale shader: {err}"))?;
        let shader = shader_ref.read();
        let pipeline = shader
            .compute_pipeline("compute_main")
            .map(|kernel| kernel.pipeline)
            .ok_or_else(|| {
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
        gpu: &RenderContext,
        source: &Texture,
        size_px: u32,
    ) -> Texture {
        self.downscale_view(context, gpu, &source.view, &source.sampler, size_px)
    }

    fn downscale_view(
        &self,
        context: &ReadOnlyAssetContext,
        gpu: &RenderContext,
        source_view: &wgpu::TextureView,
        source_sampler: &wgpu::Sampler,
        size_px: u32,
    ) -> Texture {
        let thumbnail =
            thumbnail_output_texture(context, "thumbnail_texture_downscale_output", size_px);
        let thumbnail_storage_view = thumbnail_storage_view(&thumbnail);
        let bind_group = gpu.device().create_bind_group(&wgpu::BindGroupDescriptor {
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
        let mut encoder = gpu
            .device()
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
        gpu.queue().submit(Some(encoder.finish()));
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

/// Lights a thumbnail scene. Intensities are linear: thumbnails go through
/// the default ACES tone mapping at 0 EV, like any scene without a volume.
fn add_preview_lighting(scene: &mut Scene) {
    let ambient = scene.create(None, None);
    scene.add_component(
        ambient,
        ComponentAmbientLight {
            color: Color32::from_rgb(185, 196, 210),
            intensity: 0.8,
        },
    );

    let directional = scene.create(None, None);
    let mut directional_transform = Transform::from_xyz(-4.5, 7.0, 2.25);
    directional_transform.look_at(&vec3(0.0, 0.0, 0.0));
    scene.set_world_transform(directional, directional_transform.matrix());
    scene.add_component(
        directional,
        ComponentDirectionalLight {
            color: Color32::WHITE,
            intensity: 6.0,
            ..Default::default()
        },
    );

    let point = scene.create(None, None);
    scene.set_world_transform(point, Transform::from_xyz(3.5, 2.5, -4.0).matrix());
    scene.add_component(
        point,
        ComponentPointLight {
            radius: 7.0,
            color: Color32::WHITE,
            intensity: 0.8,
        },
    );
}
