#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Instant, UNIX_EPOCH};

use eframe::wgpu;
use egui::Color32;
use egui_wgpu::RenderState;
use engine::assets::material::Material;
use engine::assets::mesh::Mesh;
use engine::assets::texture::Texture;
use engine::assets::{AssetRef, AssetRegistry};
use engine::component::{
    ComponentDirectionalLight, ComponentMesh, ComponentPointLight, ComponentSkinnedMesh,
    ComponentSkyLight,
};
use engine::context::ReadOnlyAssetContext;
use engine::math::Transform;
use engine::render::{Camera, SceneRenderer, SceneRendererOptions, Shader};
use engine::scene::{GameObject, Prefab, Scene};
use engine::utils::TypeUuid;
use nalgebra_glm::{vec3, Vec3};
use uuid::Uuid;

const THUMBNAIL_SIZE: u32 = 128;
const MAX_THUMBNAIL_JOBS_PER_FRAME: usize = 1;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ThumbnailKey {
    pub asset_id: Uuid,
    pub source_version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThumbnailRequest {
    pub asset_id: Uuid,
    pub asset_type: Uuid,
    pub source_path: Option<PathBuf>,
    pub source_version: u64,
}

impl ThumbnailRequest {
    pub fn from_asset_path(registry: &AssetRegistry, path: &Path) -> Option<Self> {
        let meta = registry.asset_meta_from_path(path)?;
        if !Self::is_supported_asset_type(meta.type_uuid) {
            return None;
        }
        Some(Self {
            asset_id: meta.id,
            asset_type: meta.type_uuid,
            source_version: source_version(meta.path.as_deref()),
            source_path: meta.path,
        })
    }

    pub fn is_supported_asset_type(asset_type: Uuid) -> bool {
        asset_type == Texture::type_uuid()
            || asset_type == Mesh::type_uuid()
            || asset_type == Prefab::type_uuid()
            || asset_type == engine::assets::skybox::Skybox::type_uuid()
    }

    pub fn key(&self) -> ThumbnailKey {
        ThumbnailKey {
            asset_id: self.asset_id,
            source_version: self.source_version,
        }
    }
}

fn source_version(path: Option<&Path>) -> u64 {
    let Some(path) = path else {
        return 0;
    };
    let Ok(metadata) = std::fs::metadata(path) else {
        return 0;
    };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| {
            duration
                .as_secs()
                .wrapping_mul(1_000_000_000)
                .wrapping_add(duration.subsec_nanos() as u64)
        })
        .unwrap_or_default();
    modified ^ metadata.len().rotate_left(32)
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ThumbnailPriority {
    Low,
    Normal,
    High,
}

impl Default for ThumbnailPriority {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThumbnailJob {
    pub request: ThumbnailRequest,
    pub priority: ThumbnailPriority,
    pub requested_at: Instant,
}

impl ThumbnailJob {
    pub fn key(&self) -> ThumbnailKey {
        self.request.key()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ThumbnailStatus {
    Missing,
    Queued { priority: ThumbnailPriority },
    InProgress,
    Ready,
    Failed { message: String },
}

#[derive(Default)]
pub struct ThumbnailPipeline {
    queued: Vec<ThumbnailJob>,
    statuses: HashMap<ThumbnailKey, ThumbnailStatus>,
}

impl ThumbnailPipeline {
    pub fn request(
        &mut self,
        request: ThumbnailRequest,
        priority: ThumbnailPriority,
    ) -> ThumbnailStatus {
        let key = request.key();
        match self.statuses.get_mut(&key) {
            Some(ThumbnailStatus::Queued { priority: existing }) => {
                if priority > *existing {
                    *existing = priority;
                    if let Some(job) = self.queued.iter_mut().find(|job| job.key() == key) {
                        job.priority = priority;
                    }
                }
            }
            Some(ThumbnailStatus::InProgress | ThumbnailStatus::Ready) => {}
            Some(ThumbnailStatus::Failed { .. } | ThumbnailStatus::Missing) | None => {
                self.queued.push(ThumbnailJob {
                    request,
                    priority,
                    requested_at: Instant::now(),
                });
                self.statuses
                    .insert(key, ThumbnailStatus::Queued { priority });
            }
        }

        self.status(key)
    }

    pub fn status(&self, key: ThumbnailKey) -> ThumbnailStatus {
        self.statuses
            .get(&key)
            .cloned()
            .unwrap_or(ThumbnailStatus::Missing)
    }

    pub fn queued_len(&self) -> usize {
        self.queued.len()
    }

    pub fn start_next(&mut self) -> Option<ThumbnailJob> {
        let index = self
            .queued
            .iter()
            .enumerate()
            .max_by_key(|(_, job)| (job.priority, std::cmp::Reverse(job.requested_at)))
            .map(|(index, _)| index)?;
        let job = self.queued.remove(index);
        self.statuses.insert(job.key(), ThumbnailStatus::InProgress);
        Some(job)
    }

    pub fn complete(&mut self, key: ThumbnailKey) -> bool {
        if !matches!(self.statuses.get(&key), Some(ThumbnailStatus::InProgress)) {
            return false;
        }
        self.statuses.insert(key, ThumbnailStatus::Ready);
        true
    }

    pub fn fail(&mut self, key: ThumbnailKey, message: impl Into<String>) -> bool {
        if !matches!(self.statuses.get(&key), Some(ThumbnailStatus::InProgress)) {
            return false;
        }
        self.statuses.insert(
            key,
            ThumbnailStatus::Failed {
                message: message.into(),
            },
        );
        true
    }

    pub fn clear(&mut self, key: ThumbnailKey) {
        self.queued.retain(|job| job.key() != key);
        self.statuses.remove(&key);
    }

    pub fn clear_asset_versions_except(&mut self, asset_id: Uuid, source_version: u64) {
        let stale_keys = self
            .statuses
            .keys()
            .copied()
            .filter(|key| key.asset_id == asset_id && key.source_version != source_version)
            .collect::<Vec<_>>();
        for key in stale_keys {
            self.clear(key);
        }
    }
}

#[derive(Default)]
pub struct ThumbnailService {
    pipeline: ThumbnailPipeline,
    textures: HashMap<ThumbnailKey, Texture>,
    texture_downscaler: Option<TextureDownscaler>,
    scene_renderer: Option<SceneRenderer>,
}

impl ThumbnailService {
    pub fn request(
        &mut self,
        request: ThumbnailRequest,
        priority: ThumbnailPriority,
    ) -> ThumbnailStatus {
        self.pipeline
            .clear_asset_versions_except(request.asset_id, request.source_version);
        self.textures.retain(|key, _| {
            key.asset_id != request.asset_id || key.source_version == request.source_version
        });
        self.pipeline.request(request, priority)
    }

    pub fn status(&self, key: ThumbnailKey) -> ThumbnailStatus {
        self.pipeline.status(key)
    }

    pub fn texture_id(&self, key: ThumbnailKey) -> Option<egui::TextureId> {
        self.textures
            .get(&key)
            .and_then(|texture| texture.handle.as_ref())
            .map(|handle| handle.id())
    }

    pub fn process(&mut self, context: &ReadOnlyAssetContext, render_state: &RenderState) {
        for _ in 0..MAX_THUMBNAIL_JOBS_PER_FRAME {
            let Some(job) = self.pipeline.start_next() else {
                return;
            };
            let key = job.key();
            match self.generate(context, render_state, &job.request) {
                Ok(texture) => {
                    self.textures.insert(key, texture);
                    self.pipeline.complete(key);
                }
                Err(message) => {
                    self.pipeline.fail(key, message);
                }
            }
        }
    }

    fn generate(
        &mut self,
        context: &ReadOnlyAssetContext,
        render_state: &RenderState,
        request: &ThumbnailRequest,
    ) -> Result<Texture, String> {
        if request.asset_type == Texture::type_uuid() {
            return self.generate_texture_thumbnail(context, render_state, request.asset_id);
        }
        if request.asset_type == Mesh::type_uuid() {
            return self.generate_mesh_thumbnail(context, render_state, request.asset_id);
        }
        if request.asset_type == Prefab::type_uuid() {
            return self.generate_prefab_thumbnail(context, render_state, request.asset_id);
        }
        if request.asset_type == engine::assets::skybox::Skybox::type_uuid() {
            return self.generate_skybox_thumbnail(context, render_state, request.asset_id);
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
        Ok(downscaler.downscale(context, render_state, &texture))
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
        let bounds = {
            let mesh = mesh_ref.read();
            Bounds::from_mesh(&mesh).unwrap_or_default()
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
        add_preview_lighting(context, &mut scene);
        self.render_scene_thumbnail(context, render_state, &scene, bounds)
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
        let bounds = scene_mesh_bounds(context, &scene, root).unwrap_or_default();
        add_preview_lighting(context, &mut scene);
        self.render_scene_thumbnail(context, render_state, &scene, bounds)
    }

    fn generate_skybox_thumbnail(
        &mut self,
        context: &ReadOnlyAssetContext,
        render_state: &RenderState,
        asset_id: Uuid,
    ) -> Result<Texture, String> {
        context
            .registries
            .assets
            .read()
            .load_by_id::<engine::assets::skybox::Skybox>(asset_id)
            .map_err(|err| format!("failed to load skybox thumbnail source: {err}"))?;
        let mut scene = context.scene();
        let sky_light = scene.create(None, None);
        scene.add_component(
            sky_light,
            ComponentSkyLight {
                active: true,
                intensity: 1.0,
                skybox: AssetRef::from_id(asset_id),
            },
        );
        self.render_scene_thumbnail(context, render_state, &scene, Bounds::unit())
    }

    fn render_scene_thumbnail(
        &mut self,
        context: &ReadOnlyAssetContext,
        render_state: &RenderState,
        scene: &Scene,
        bounds: Bounds,
    ) -> Result<Texture, String> {
        let (camera, camera_transform) = camera_for_bounds(bounds);
        let renderer = self.scene_renderer.get_or_insert_with(|| {
            SceneRenderer::new(
                context,
                SceneRendererOptions {
                    grid: false,
                    gizmos: false,
                    samples: 1,
                    clear_color: Color32::from_rgb(18, 20, 22),
                },
                (THUMBNAIL_SIZE, THUMBNAIL_SIZE),
            )
        });
        renderer.resize_textures(THUMBNAIL_SIZE, THUMBNAIL_SIZE);
        renderer.render_scene_base(render_state, &camera, &camera_transform, scene, None);
        renderer.finalize_scene(render_state);

        let source = renderer.scene_texture();
        let thumbnail = Texture::new(
            context.render_context.clone(),
            &wgpu::TextureDescriptor {
                label: Some("thumbnail_scene_texture"),
                size: wgpu::Extent3d {
                    width: THUMBNAIL_SIZE,
                    height: THUMBNAIL_SIZE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: source.descriptor.format,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            None,
            None,
            true,
        );
        let mut encoder =
            render_state
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("thumbnail_scene_copy"),
                });
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &source.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &thumbnail.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: THUMBNAIL_SIZE,
                height: THUMBNAIL_SIZE,
                depth_or_array_layers: 1,
            },
        );
        render_state.queue.submit(Some(encoder.finish()));
        Ok(thumbnail)
    }
}

struct TextureDownscaler {
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
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
    ) -> Texture {
        let thumbnail = Texture::new(
            context.render_context.clone(),
            &wgpu::TextureDescriptor {
                label: Some("thumbnail_texture_downscale_output"),
                size: wgpu::Extent3d {
                    width: THUMBNAIL_SIZE,
                    height: THUMBNAIL_SIZE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            None,
            None,
            true,
        );
        let bind_group = render_state
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("thumbnail_downscale_bind_group"),
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
                        resource: wgpu::BindingResource::TextureView(&thumbnail.view),
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
            compute_pass.dispatch_workgroups(
                THUMBNAIL_SIZE.div_ceil(8),
                THUMBNAIL_SIZE.div_ceil(8),
                1,
            );
        }
        render_state.queue.submit(Some(encoder.finish()));
        thumbnail
    }
}

#[derive(Clone, Copy, Debug)]
struct Bounds {
    min: Vec3,
    max: Vec3,
}

impl Default for Bounds {
    fn default() -> Self {
        Self::unit()
    }
}

impl Bounds {
    fn unit() -> Self {
        Self {
            min: vec3(-0.5, -0.5, -0.5),
            max: vec3(0.5, 0.5, 0.5),
        }
    }

    fn from_mesh(mesh: &Mesh) -> Option<Self> {
        let mut vertices = mesh.vertices.iter();
        let first = *vertices.next()?;
        let mut bounds = Self {
            min: first,
            max: first,
        };
        for vertex in vertices {
            bounds.include(*vertex);
        }
        Some(bounds)
    }

    fn include_mesh(&mut self, mesh: &Mesh, transform: &Transform) {
        for vertex in &mesh.vertices {
            self.include(transform.transform_position(vertex));
        }
    }

    fn include(&mut self, point: Vec3) {
        self.min.x = self.min.x.min(point.x);
        self.min.y = self.min.y.min(point.y);
        self.min.z = self.min.z.min(point.z);
        self.max.x = self.max.x.max(point.x);
        self.max.y = self.max.y.max(point.y);
        self.max.z = self.max.z.max(point.z);
    }

    fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    fn radius(&self) -> f32 {
        let extents = self.max - self.min;
        extents.norm().max(0.5) * 0.5
    }
}

fn scene_mesh_bounds(
    context: &ReadOnlyAssetContext,
    scene: &Scene,
    root: GameObject,
) -> Option<Bounds> {
    let mut bounds = None;
    for game_object in std::iter::once(root).chain(scene.descendants(root)) {
        let transform = scene.world_transform(game_object);
        if let Some(mesh_ref) = scene
            .read_component::<ComponentMesh, _, _>(game_object, |component| {
                component.mesh.get_ref(&context.registries)
            })
            .flatten()
        {
            let mesh = mesh_ref.read();
            let bounds = bounds.get_or_insert_with(Bounds::unit);
            bounds.include_mesh(&mesh, &transform);
        }
        if let Some(mesh_ref) = scene
            .read_component::<ComponentSkinnedMesh, _, _>(game_object, |component| {
                component.mesh.get_ref(&context.registries)
            })
            .flatten()
        {
            let mesh = mesh_ref.read();
            let bounds = bounds.get_or_insert_with(Bounds::unit);
            bounds.include_mesh(&mesh, &transform);
        }
    }
    bounds
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

fn add_preview_lighting(context: &ReadOnlyAssetContext, scene: &mut Scene) {
    if let Some(skybox_id) = first_skybox_id(context) {
        let sky_light = scene.create(None, None);
        scene.add_component(
            sky_light,
            ComponentSkyLight {
                active: true,
                intensity: 0.75,
                skybox: AssetRef::from_id(skybox_id),
            },
        );
    }

    let directional = scene.create(None, None);
    let mut directional_transform = Transform::from_xyz(-2.0, 3.0, -4.0);
    directional_transform.look_at(&vec3(0.0, 0.0, 0.0));
    scene.set_world_transform(directional, directional_transform.matrix());
    scene.add_component(
        directional,
        ComponentDirectionalLight {
            active: true,
            color: Color32::WHITE,
            intensity: 0.65,
        },
    );

    let point = scene.create(None, None);
    scene.set_world_transform(point, Transform::from_xyz(2.5, 3.0, -3.0).matrix());
    scene.add_component(
        point,
        ComponentPointLight {
            active: true,
            radius: 8.0,
            color: Color32::WHITE,
            intensity: 0.8,
        },
    );
}

fn first_skybox_id(context: &ReadOnlyAssetContext) -> Option<Uuid> {
    let mut assets = Vec::new();
    context.registries.assets.read().search_assets(
        "",
        Some(engine::assets::skybox::Skybox::type_uuid()),
        &mut assets,
    );
    assets.first().map(|meta| meta.id)
}

fn camera_for_bounds(bounds: Bounds) -> (Camera, Transform) {
    let center = bounds.center();
    let radius = bounds.radius();
    let distance = (radius / (25.0f32.to_radians()).tan()).max(1.5) * 1.35;
    let camera_position = center + vec3(0.75, 0.45, -1.0).normalize() * distance;
    let mut camera_transform =
        Transform::from_xyz(camera_position.x, camera_position.y, camera_position.z);
    camera_transform.look_at(&center);
    let camera = Camera::new(1.0, 50.0f32.to_radians(), 0.01, distance + radius * 4.0);
    (camera, camera_transform)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(source_version: u64) -> ThumbnailRequest {
        ThumbnailRequest {
            asset_id: Uuid::from_u128(1),
            asset_type: Uuid::from_u128(2),
            source_path: Some(PathBuf::from("assets/texture.png")),
            source_version,
        }
    }

    #[test]
    fn request_deduplicates_and_promotes_priority() {
        let mut pipeline = ThumbnailPipeline::default();
        let request = request(7);
        let key = request.key();

        pipeline.request(request.clone(), ThumbnailPriority::Low);
        pipeline.request(request, ThumbnailPriority::High);

        assert_eq!(pipeline.queued_len(), 1);
        assert_eq!(
            pipeline.status(key),
            ThumbnailStatus::Queued {
                priority: ThumbnailPriority::High
            }
        );
    }

    #[test]
    fn start_next_picks_highest_priority_job() {
        let mut pipeline = ThumbnailPipeline::default();
        let low = request(1);
        let high = request(2);

        pipeline.request(low.clone(), ThumbnailPriority::Low);
        pipeline.request(high.clone(), ThumbnailPriority::High);

        let job = pipeline.start_next().unwrap();

        assert_eq!(job.key(), high.key());
        assert_eq!(pipeline.status(high.key()), ThumbnailStatus::InProgress);
        assert_eq!(
            pipeline.status(low.key()),
            ThumbnailStatus::Queued {
                priority: ThumbnailPriority::Low
            }
        );
    }

    #[test]
    fn complete_and_fail_only_update_in_progress_jobs() {
        let mut pipeline = ThumbnailPipeline::default();
        let request = request(1);
        let key = request.key();

        assert!(!pipeline.complete(key));
        assert!(!pipeline.fail(key, "not started"));

        pipeline.request(request.clone(), ThumbnailPriority::Normal);
        assert!(!pipeline.complete(key));

        pipeline.start_next();
        assert!(pipeline.complete(key));
        assert_eq!(pipeline.status(key), ThumbnailStatus::Ready);

        pipeline.request(request, ThumbnailPriority::Normal);
        assert!(!pipeline.fail(key, "already ready"));
        assert_eq!(pipeline.status(key), ThumbnailStatus::Ready);
    }

    #[test]
    fn clear_prevents_stale_in_progress_completion() {
        let mut pipeline = ThumbnailPipeline::default();
        let request = request(1);
        let key = request.key();

        pipeline.request(request, ThumbnailPriority::Normal);
        pipeline.start_next();
        pipeline.clear(key);

        assert!(!pipeline.complete(key));
        assert_eq!(pipeline.status(key), ThumbnailStatus::Missing);
    }

    #[test]
    fn clear_asset_versions_removes_old_versions() {
        let mut pipeline = ThumbnailPipeline::default();
        let old = request(1);
        let new = request(2);
        let old_key = old.key();
        let new_key = new.key();

        pipeline.request(old, ThumbnailPriority::Normal);
        pipeline.request(new, ThumbnailPriority::Normal);
        pipeline.clear_asset_versions_except(Uuid::from_u128(1), 2);

        assert_eq!(pipeline.status(old_key), ThumbnailStatus::Missing);
        assert_eq!(
            pipeline.status(new_key),
            ThumbnailStatus::Queued {
                priority: ThumbnailPriority::Normal
            }
        );
    }
}
