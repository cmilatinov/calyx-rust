#![allow(dead_code)]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Instant, UNIX_EPOCH};

use eframe::wgpu;
use egui::Color32;
use egui_wgpu::RenderState;
use engine::assets::material::Material;
use engine::assets::mesh::Mesh;
use engine::assets::skybox::Skybox;
use engine::assets::texture::Texture;
use engine::assets::{AssetRef, AssetRegistry};
use engine::component::{
    ComponentAmbientLight, ComponentDirectionalLight, ComponentMesh, ComponentPointLight,
    ComponentSkinnedMesh,
};
use engine::context::ReadOnlyAssetContext;
use engine::math::Transform;
use engine::render::{Camera, SceneRenderer, SceneRendererOptions, Shader};
use engine::scene::{GameObject, Prefab, Scene};
use engine::utils::TypeUuid;
use image::{ColorType, DynamicImage, ImageBuffer, ImageFormat, ImageReader, RgbaImage};
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec3, Vec3};
use sha1::{Digest, Sha1};
use uuid::Uuid;

const THUMBNAIL_SIZE: u32 = 128;
const THUMBNAIL_CACHE_VERSION: u32 = 3;
const THUMBNAIL_MAX_FAILURES: u8 = 3;

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
            || asset_type == Material::type_uuid()
            || asset_type == Mesh::type_uuid()
            || asset_type == Prefab::type_uuid()
            || asset_type == Skybox::type_uuid()
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
    let Ok(metadata) = fs::metadata(path) else {
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

fn thumbnail_asset_type_name(asset_type: Uuid) -> &'static str {
    if asset_type == Texture::type_uuid() {
        "texture"
    } else if asset_type == Material::type_uuid() {
        "material"
    } else if asset_type == Mesh::type_uuid() {
        "mesh"
    } else if asset_type == Prefab::type_uuid() {
        "prefab"
    } else if asset_type == Skybox::type_uuid() {
        "skybox"
    } else {
        "unknown"
    }
}

fn thumbnail_source_label(request: &ThumbnailRequest) -> String {
    request
        .source_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "<unknown>".into())
}

struct ThumbnailCache {
    root: PathBuf,
}

impl ThumbnailCache {
    fn new(context: &ReadOnlyAssetContext) -> Self {
        let asset_root = context.registries.assets.read().root_path().clone();
        let project_hash = project_cache_hash(&asset_root);
        let cache_root = dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("Calyx")
            .join("Editor")
            .join("thumbnails")
            .join(project_hash)
            .join(format!("v{THUMBNAIL_CACHE_VERSION}"))
            .join(format!("{THUMBNAIL_SIZE}px"));

        Self { root: cache_root }
    }

    fn load(
        &self,
        context: &ReadOnlyAssetContext,
        request: &ThumbnailRequest,
    ) -> Result<Option<Texture>, String> {
        let path = self.path(request);
        if !path.is_file() {
            return Ok(None);
        }

        let image = image::open(&path)
            .map_err(|err| {
                format!(
                    "failed to decode cached thumbnail {}: {err}",
                    path.display()
                )
            })?
            .to_rgba8();
        if image.width() != THUMBNAIL_SIZE || image.height() != THUMBNAIL_SIZE {
            return Err(format!(
                "cached thumbnail {} has size {}x{}, expected {}x{}",
                path.display(),
                image.width(),
                image.height(),
                THUMBNAIL_SIZE,
                THUMBNAIL_SIZE
            ));
        }

        Ok(Some(texture_from_rgba8(
            context,
            "thumbnail_disk_cache",
            &image,
        )))
    }

    fn store(
        &self,
        render_state: &RenderState,
        request: &ThumbnailRequest,
        texture: &Texture,
    ) -> Result<(), String> {
        let pixels = read_texture_rgba8(render_state, texture)?;
        let image: RgbaImage = ImageBuffer::from_vec(THUMBNAIL_SIZE, THUMBNAIL_SIZE, pixels)
            .ok_or_else(|| "thumbnail readback returned an unexpected byte count".to_string())?;
        let path = self.path(request);
        let parent = path
            .parent()
            .ok_or_else(|| format!("thumbnail cache path has no parent: {}", path.display()))?;
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create thumbnail cache directory {}: {err}",
                parent.display()
            )
        })?;

        let tmp_path = path.with_extension("png.tmp");
        if tmp_path.exists() {
            fs::remove_file(&tmp_path).map_err(|err| {
                format!(
                    "failed to remove stale thumbnail cache temp file {}: {err}",
                    tmp_path.display()
                )
            })?;
        }

        image
            .save_with_format(&tmp_path, ImageFormat::Png)
            .map_err(|err| {
                format!(
                    "failed to encode thumbnail cache file {}: {err}",
                    tmp_path.display()
                )
            })?;

        match fs::rename(&tmp_path, &path) {
            Ok(()) => Ok(()),
            Err(rename_error) if path.exists() => {
                fs::remove_file(&path).map_err(|err| {
                    format!(
                        "failed to replace thumbnail cache file {} after rename error {rename_error}: {err}",
                        path.display()
                    )
                })?;
                fs::rename(&tmp_path, &path).map_err(|err| {
                    format!(
                        "failed to move thumbnail cache file {} to {}: {err}",
                        tmp_path.display(),
                        path.display()
                    )
                })
            }
            Err(err) => Err(format!(
                "failed to move thumbnail cache file {} to {}: {err}",
                tmp_path.display(),
                path.display()
            )),
        }
    }

    fn path(&self, request: &ThumbnailRequest) -> PathBuf {
        self.root
            .join(thumbnail_asset_type_name(request.asset_type))
            .join(format!(
                "{}-{:016x}.png",
                request.asset_id, request.source_version
            ))
    }
}

fn project_cache_hash(asset_root: &Path) -> String {
    let canonical = fs::canonicalize(asset_root).unwrap_or_else(|_| asset_root.to_path_buf());
    let normalized = canonical.to_string_lossy().replace('\\', "/");
    let mut hasher = Sha1::new();
    hasher.update(normalized.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn texture_from_image_file(
    context: &ReadOnlyAssetContext,
    label: &str,
    path: &Path,
) -> Result<Texture, String> {
    let reader = ImageReader::open(path)
        .map_err(|err| format!("failed to open texture source {}: {err}", path.display()))?;
    let image = transform_thumbnail_source_image(
        reader
            .decode()
            .map_err(|err| format!("failed to decode texture source {}: {err}", path.display()))?,
    );
    let texture_depth = image.color().bytes_per_pixel() as u32;
    let texture_format = thumbnail_source_texture_format(image.color());
    let texture_size = wgpu::Extent3d {
        width: image.width(),
        height: image.height(),
        depth_or_array_layers: 1,
    };
    let texture = Texture::new(
        context.render_context.clone(),
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: texture_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: texture_format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        None,
        None,
        false,
    );
    context.render_context.queue().write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture.texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        image.as_bytes(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(texture_depth * image.width()),
            rows_per_image: Some(image.height()),
        },
        texture_size,
    );
    Ok(texture)
}

fn transform_thumbnail_source_image(image: DynamicImage) -> DynamicImage {
    match image.color() {
        ColorType::Rgba32F | ColorType::Rgba8 => image,
        ColorType::Rgb32F => image.to_rgba32f().into(),
        _ => image.to_rgba8().into(),
    }
}

fn thumbnail_source_texture_format(color: ColorType) -> wgpu::TextureFormat {
    match color {
        ColorType::Rgba32F => wgpu::TextureFormat::Rgba32Float,
        _ => wgpu::TextureFormat::Rgba8Unorm,
    }
}

fn texture_from_rgba8(context: &ReadOnlyAssetContext, label: &str, image: &RgbaImage) -> Texture {
    let size = wgpu::Extent3d {
        width: image.width(),
        height: image.height(),
        depth_or_array_layers: 1,
    };
    let texture = Texture::new(
        context.render_context.clone(),
        &wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        },
        Some(wgpu::SamplerDescriptor {
            label: Some(label),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        }),
        None,
        true,
    );
    context.render_context.queue().write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture.texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        image.as_raw(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(image.width() * 4),
            rows_per_image: Some(image.height()),
        },
        size,
    );
    texture
}

fn read_texture_rgba8(render_state: &RenderState, texture: &Texture) -> Result<Vec<u8>, String> {
    if texture.descriptor.format != wgpu::TextureFormat::Rgba8Unorm {
        return Err(format!(
            "thumbnail cache requires Rgba8Unorm textures, got {:?}",
            texture.descriptor.format
        ));
    }
    if texture.descriptor.dimension != wgpu::TextureDimension::D2
        || texture.descriptor.size.depth_or_array_layers != 1
        || texture.descriptor.sample_count != 1
    {
        return Err("thumbnail cache requires a single-sample 2D texture".into());
    }

    let width = texture.descriptor.size.width;
    let height = texture.descriptor.size.height;
    if width != THUMBNAIL_SIZE || height != THUMBNAIL_SIZE {
        return Err(format!(
            "thumbnail cache requires {}x{} textures, got {}x{}",
            THUMBNAIL_SIZE, THUMBNAIL_SIZE, width, height
        ));
    }

    let bytes_per_pixel = 4;
    let unpadded_bytes_per_row = width * bytes_per_pixel;
    let padded_bytes_per_row = align_to(unpadded_bytes_per_row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let output_buffer_size = padded_bytes_per_row as u64 * height as u64;
    let output_buffer = render_state.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("thumbnail_cache_readback"),
        size: output_buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = render_state
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("thumbnail_cache_readback"),
        });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture.texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &output_buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    render_state.queue.submit(Some(encoder.finish()));

    let slice = output_buffer.slice(..);
    let (tx, rx) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });

    loop {
        let _ = render_state.device.poll(wgpu::Maintain::Poll);
        match rx.try_recv() {
            Ok(Ok(())) => break,
            Ok(Err(err)) => return Err(format!("thumbnail cache readback failed: {err:?}")),
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err("thumbnail cache readback disconnected".into());
            }
            Err(mpsc::TryRecvError::Empty) => thread::yield_now(),
        }
    }

    let data = slice.get_mapped_range();
    let mut pixels = vec![0; (unpadded_bytes_per_row * height) as usize];
    for row in 0..height as usize {
        let source_offset = row * padded_bytes_per_row as usize;
        let target_offset = row * unpadded_bytes_per_row as usize;
        let source = &data[source_offset..source_offset + unpadded_bytes_per_row as usize];
        let target = &mut pixels[target_offset..target_offset + unpadded_bytes_per_row as usize];
        target.copy_from_slice(source);
    }
    drop(data);
    output_buffer.unmap();

    Ok(pixels)
}

fn align_to(value: u32, alignment: u32) -> u32 {
    value.div_ceil(alignment) * alignment
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
    failure_counts: HashMap<ThumbnailKey, u8>,
}

impl ThumbnailPipeline {
    pub fn request(
        &mut self,
        request: ThumbnailRequest,
        priority: ThumbnailPriority,
    ) -> ThumbnailStatus {
        let key = request.key();
        match self.statuses.get(&key).cloned() {
            Some(ThumbnailStatus::Queued { priority: existing }) => {
                if priority > existing {
                    if let Some(ThumbnailStatus::Queued { priority: existing }) =
                        self.statuses.get_mut(&key)
                    {
                        *existing = priority;
                    }
                    if let Some(job) = self.queued.iter_mut().find(|job| job.key() == key) {
                        job.priority = priority;
                    }
                }
            }
            Some(ThumbnailStatus::InProgress | ThumbnailStatus::Ready) => {}
            Some(ThumbnailStatus::Failed { .. })
                if self.failure_count(key) < THUMBNAIL_MAX_FAILURES =>
            {
                self.enqueue(request, priority);
            }
            Some(ThumbnailStatus::Failed { .. }) => {}
            Some(ThumbnailStatus::Missing) | None => {
                self.enqueue(request, priority);
            }
        }

        self.status(key)
    }

    fn enqueue(&mut self, request: ThumbnailRequest, priority: ThumbnailPriority) {
        let key = request.key();
        self.queued.push(ThumbnailJob {
            request,
            priority,
            requested_at: Instant::now(),
        });
        self.statuses
            .insert(key, ThumbnailStatus::Queued { priority });
    }

    pub fn status(&self, key: ThumbnailKey) -> ThumbnailStatus {
        self.statuses
            .get(&key)
            .cloned()
            .unwrap_or(ThumbnailStatus::Missing)
    }

    fn failure_count(&self, key: ThumbnailKey) -> u8 {
        self.failure_counts.get(&key).copied().unwrap_or_default()
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
        self.failure_counts.remove(&key);
        self.statuses.insert(key, ThumbnailStatus::Ready);
        true
    }

    pub fn fail(&mut self, key: ThumbnailKey, message: impl Into<String>) -> bool {
        if !matches!(self.statuses.get(&key), Some(ThumbnailStatus::InProgress)) {
            return false;
        }
        let count = self.failure_counts.entry(key).or_default();
        *count = count.saturating_add(1);
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
        self.failure_counts.remove(&key);
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

pub struct ThumbnailService {
    shared: Arc<ThumbnailShared>,
    worker: Option<JoinHandle<()>>,
}

struct ThumbnailShared {
    state: Mutex<ThumbnailState>,
    wake: Condvar,
}

#[derive(Default)]
struct ThumbnailState {
    pipeline: ThumbnailPipeline,
    textures: HashMap<ThumbnailKey, Texture>,
    stop: bool,
}

#[derive(Default)]
struct ThumbnailGenerator {
    texture_downscaler: Option<TextureDownscaler>,
    scene_renderer: Option<SceneRenderer>,
    skybox_front_face_downscaler: Option<SkyboxFrontFaceDownscaler>,
}

impl Default for ThumbnailService {
    fn default() -> Self {
        Self {
            shared: Arc::new(ThumbnailShared {
                state: Mutex::new(ThumbnailState::default()),
                wake: Condvar::new(),
            }),
            worker: None,
        }
    }
}

impl ThumbnailService {
    pub fn request(
        &mut self,
        request: ThumbnailRequest,
        priority: ThumbnailPriority,
    ) -> ThumbnailStatus {
        let mut state = self.shared.state.lock().unwrap();
        state
            .pipeline
            .clear_asset_versions_except(request.asset_id, request.source_version);
        state.textures.retain(|key, _| {
            key.asset_id != request.asset_id || key.source_version == request.source_version
        });
        let status = state.pipeline.request(request, priority);
        drop(state);
        self.shared.wake.notify_one();
        status
    }

    pub fn status(&self, key: ThumbnailKey) -> ThumbnailStatus {
        self.shared.state.lock().unwrap().pipeline.status(key)
    }

    pub fn texture_id(&self, key: ThumbnailKey) -> Option<egui::TextureId> {
        self.shared
            .state
            .lock()
            .unwrap()
            .textures
            .get(&key)
            .and_then(|texture| texture.handle.as_ref())
            .map(|handle| handle.id())
    }

    pub fn process(&mut self, context: &ReadOnlyAssetContext, _render_state: &RenderState) {
        self.ensure_worker(context.clone());
    }

    fn ensure_worker(&mut self, context: ReadOnlyAssetContext) {
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            return;
        }
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                log::warn!("Thumbnail generation worker exited with a panic");
            }
        }

        let shared = self.shared.clone();
        self.worker = Some(
            thread::Builder::new()
                .name("thumbnail-generator".into())
                .spawn(move || thumbnail_worker_loop(shared, context))
                .expect("failed to spawn thumbnail generation worker"),
        );
        self.shared.wake.notify_one();
    }
}

impl Drop for ThumbnailService {
    fn drop(&mut self) {
        {
            let mut state = self.shared.state.lock().unwrap();
            state.stop = true;
        }
        self.shared.wake.notify_one();
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                log::warn!("Thumbnail generation worker exited with a panic");
            }
        }
    }
}

fn thumbnail_worker_loop(shared: Arc<ThumbnailShared>, context: ReadOnlyAssetContext) {
    let cache = ThumbnailCache::new(&context);
    let mut generator = ThumbnailGenerator::default();
    loop {
        let job = {
            let mut state = shared.state.lock().unwrap();
            loop {
                if state.stop {
                    return;
                }
                if let Some(job) = state.pipeline.start_next() {
                    break job;
                }
                state = shared.wake.wait(state).unwrap();
            }
        };

        let key = job.key();
        let started_at = Instant::now();
        let render_state = context.render_context.render_state();
        match cache.load(&context, &job.request) {
            Ok(Some(texture)) => {
                let mut state = shared.state.lock().unwrap();
                let status_updated = state.pipeline.complete(key);
                if status_updated {
                    state.textures.insert(key, texture);
                }
                continue;
            }
            Ok(None) | Err(_) => {}
        }

        match generator.generate(&context, render_state, &job.request) {
            Ok(texture) => {
                let _ = cache.store(render_state, &job.request, &texture);
                let mut state = shared.state.lock().unwrap();
                let status_updated = state.pipeline.complete(key);
                if status_updated {
                    log::trace!(
                        "Generated thumbnail asset={} type={} version={} path={} texture_size={}x{} format={:?} elapsed_ms={}",
                        job.request.asset_id,
                        thumbnail_asset_type_name(job.request.asset_type),
                        job.request.source_version,
                        thumbnail_source_label(&job.request),
                        texture.descriptor.size.width,
                        texture.descriptor.size.height,
                        texture.descriptor.format,
                        started_at.elapsed().as_millis()
                    );
                    state.textures.insert(key, texture);
                }
            }
            Err(message) => {
                let mut state = shared.state.lock().unwrap();
                let status_updated = state.pipeline.fail(key, message.as_str());
                let failure_count = state.pipeline.failure_count(key);
                if status_updated && failure_count <= THUMBNAIL_MAX_FAILURES {
                    log::warn!(
                        "Thumbnail request failed asset={} type={} version={} attempt={}/{} path={} elapsed_ms={} error={}",
                        job.request.asset_id,
                        thumbnail_asset_type_name(job.request.asset_type),
                        job.request.source_version,
                        failure_count,
                        THUMBNAIL_MAX_FAILURES,
                        thumbnail_source_label(&job.request),
                        started_at.elapsed().as_millis(),
                        message
                    );
                }
            }
        }
    }
}

impl ThumbnailGenerator {
    fn generate(
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
        Ok(downscaler.downscale(context, render_state, &texture))
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
        let bounds = {
            let sphere = sphere_ref.read();
            Bounds::from_mesh(&sphere).unwrap_or_default()
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
        self.render_scene_thumbnail(context, render_state, &scene, bounds)
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
        add_preview_lighting(&mut scene);
        self.render_scene_thumbnail(context, render_state, &scene, bounds)
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
        Ok(downscaler.downscale_front_face(context, render_state, &source))
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
        add_preview_lighting(&mut scene);
        self.render_scene_thumbnail(context, render_state, &scene, bounds)
    }

    fn render_scene_thumbnail(
        &mut self,
        context: &ReadOnlyAssetContext,
        render_state: &RenderState,
        scene: &Scene,
        bounds: Bounds,
    ) -> Result<Texture, String> {
        let (camera, camera_transform) = camera_for_bounds(bounds);
        {
            let renderer = self.scene_renderer.get_or_insert_with(|| {
                SceneRenderer::new(
                    context,
                    SceneRendererOptions {
                        grid: false,
                        gizmos: false,
                        samples: 1,
                        clear_color: Color32::TRANSPARENT,
                    },
                    (THUMBNAIL_SIZE, THUMBNAIL_SIZE),
                )
            });
            renderer.resize_textures(THUMBNAIL_SIZE, THUMBNAIL_SIZE);
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
        Ok(downscaler.downscale(context, render_state, renderer.scene_texture()))
    }
}

struct SkyboxFrontFaceDownscaler {
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

impl SkyboxFrontFaceDownscaler {
    fn new(context: &ReadOnlyAssetContext) -> Result<Self, String> {
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
    ) -> Texture {
        let thumbnail = thumbnail_output_texture(context, "thumbnail_skybox_front_face_output");
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
                        resource: wgpu::BindingResource::TextureView(&thumbnail.view),
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

struct TextureDownscaler {
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

fn thumbnail_output_texture(context: &ReadOnlyAssetContext, label: &'static str) -> Texture {
    Texture::new(
        context.render_context.clone(),
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: THUMBNAIL_SIZE,
                height: THUMBNAIL_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        },
        None,
        None,
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
    ) -> Texture {
        self.downscale_view(context, render_state, &source.view, &source.sampler)
    }

    fn downscale_view(
        &self,
        context: &ReadOnlyAssetContext,
        render_state: &RenderState,
        source_view: &wgpu::TextureView,
        source_sampler: &wgpu::Sampler,
    ) -> Texture {
        let thumbnail = thumbnail_output_texture(context, "thumbnail_texture_downscale_output");
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

    fn corners(&self) -> [Vec3; 8] {
        [
            vec3(self.min.x, self.min.y, self.min.z),
            vec3(self.max.x, self.min.y, self.min.z),
            vec3(self.min.x, self.max.y, self.min.z),
            vec3(self.max.x, self.max.y, self.min.z),
            vec3(self.min.x, self.min.y, self.max.z),
            vec3(self.max.x, self.min.y, self.max.z),
            vec3(self.min.x, self.max.y, self.max.z),
            vec3(self.max.x, self.max.y, self.max.z),
        ]
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

fn add_preview_lighting(scene: &mut Scene) {
    let ambient = scene.create(None, None);
    scene.add_component(
        ambient,
        ComponentAmbientLight {
            active: true,
            color: Color32::from_rgb(210, 220, 232),
            intensity: 0.22,
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

fn camera_for_bounds(bounds: Bounds) -> (Camera, Transform) {
    const ASPECT: f32 = 1.0;
    const FOV_X: f32 = 40.0f32.to_radians();
    const FRAME_MARGIN: f32 = 1.2;
    const MIN_DISTANCE: f32 = 1.2;

    let center = bounds.center();
    let radius = bounds.radius();
    let view_direction = vec3(0.9, 0.55, -0.85).normalize();
    let unit_transform = thumbnail_camera_transform(center, view_direction, 1.0);

    let tan_half_x = (FOV_X * 0.5).tan();
    let tan_half_y = (FOV_X * 0.5).tan();
    let mut distance = MIN_DISTANCE.max(radius);
    for corner in bounds.corners() {
        let offset = corner - center;
        let view_offset = unit_transform.inverse_transform_direction(&offset);
        distance = distance.max(view_offset.x.abs() / tan_half_x - view_offset.z);
        distance = distance.max(view_offset.y.abs() / tan_half_y - view_offset.z);
        distance = distance.max(0.01 - view_offset.z);
    }
    distance = (distance * FRAME_MARGIN).max(MIN_DISTANCE);

    let camera_transform = thumbnail_camera_transform(center, view_direction, distance);
    let max_depth = bounds
        .corners()
        .into_iter()
        .map(|corner| camera_transform.inverse_transform_position(&corner).z)
        .fold(distance, f32::max);
    let camera = Camera::new(ASPECT, FOV_X, 0.01, max_depth + radius.max(1.0));
    (camera, camera_transform)
}

fn thumbnail_camera_transform(center: Vec3, view_direction: Vec3, distance: f32) -> Transform {
    let camera_position = center + view_direction * distance;
    let target_direction = (center - camera_position).normalize();
    Transform::from_components(
        camera_position,
        UnitQuaternion::face_towards(&target_direction, &Vec3::y_axis()),
        vec3(1.0, 1.0, 1.0),
    )
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
    fn failed_jobs_retry_until_failure_threshold() {
        let mut pipeline = ThumbnailPipeline::default();
        let request = request(1);
        let key = request.key();

        pipeline.request(request.clone(), ThumbnailPriority::Normal);

        for attempt in 1..=THUMBNAIL_MAX_FAILURES {
            pipeline.start_next();
            assert!(pipeline.fail(key, "bad source"));
            assert_eq!(pipeline.failure_count(key), attempt);

            let status = pipeline.request(request.clone(), ThumbnailPriority::High);
            if attempt < THUMBNAIL_MAX_FAILURES {
                assert_eq!(pipeline.queued_len(), 1);
                assert_eq!(
                    status,
                    ThumbnailStatus::Queued {
                        priority: ThumbnailPriority::High
                    }
                );
            } else {
                assert_eq!(pipeline.queued_len(), 0);
                assert_eq!(
                    status,
                    ThumbnailStatus::Failed {
                        message: "bad source".into()
                    }
                );
            }
        }
    }

    #[test]
    fn complete_and_clear_reset_failure_count() {
        let mut pipeline = ThumbnailPipeline::default();
        let request = request(1);
        let key = request.key();

        pipeline.request(request.clone(), ThumbnailPriority::Normal);
        pipeline.start_next();
        assert!(pipeline.fail(key, "bad source"));
        assert_eq!(pipeline.failure_count(key), 1);

        pipeline.request(request.clone(), ThumbnailPriority::Normal);
        pipeline.start_next();
        assert!(pipeline.complete(key));
        assert_eq!(pipeline.failure_count(key), 0);

        pipeline.request(request, ThumbnailPriority::Normal);
        pipeline.clear(key);
        assert_eq!(pipeline.failure_count(key), 0);
        assert_eq!(pipeline.status(key), ThumbnailStatus::Missing);
    }

    fn assert_camera_encloses(bounds: Bounds) {
        let (camera, transform) = camera_for_bounds(bounds);
        let tan_half_x = (camera.fov_x * 0.5).tan();
        let tan_half_y = (camera.fov_x * 0.5).tan();

        for corner in bounds.corners() {
            let view = transform.inverse_transform_position(&corner);
            let depth = view.z;
            assert!(
                depth >= camera.near_plane,
                "corner {corner:?} is before the near plane at view-space {view:?}"
            );
            assert!(
                depth <= camera.far_plane,
                "corner {corner:?} is beyond the far plane at view-space {view:?}"
            );
            assert!(
                view.x.abs() <= depth * tan_half_x + 0.001,
                "corner {corner:?} is outside horizontal frustum at view-space {view:?}"
            );
            assert!(
                view.y.abs() <= depth * tan_half_y + 0.001,
                "corner {corner:?} is outside vertical frustum at view-space {view:?}"
            );
        }
    }

    #[test]
    fn thumbnail_camera_encloses_bounds() {
        assert_camera_encloses(Bounds {
            min: vec3(-5.0, -0.25, -0.25),
            max: vec3(5.0, 0.25, 0.25),
        });
        assert_camera_encloses(Bounds {
            min: vec3(-0.25, -6.0, -0.25),
            max: vec3(0.25, 6.0, 0.25),
        });
        assert_camera_encloses(Bounds {
            min: vec3(-0.5, -0.5, -4.0),
            max: vec3(0.5, 0.5, 4.0),
        });
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

    #[test]
    fn material_and_skybox_assets_are_supported_for_thumbnails() {
        assert!(ThumbnailRequest::is_supported_asset_type(
            Material::type_uuid()
        ));
        assert_eq!(thumbnail_asset_type_name(Material::type_uuid()), "material");
        assert!(ThumbnailRequest::is_supported_asset_type(
            Skybox::type_uuid()
        ));
        assert_eq!(thumbnail_asset_type_name(Skybox::type_uuid()), "skybox");
    }

    #[test]
    fn skybox_front_face_shader_builds() {
        let assets_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets");
        let assets_path = dunce::canonicalize(assets_path).expect("assets dir not found");
        let context = engine::test_support::test_asset_context_with_assets(vec![assets_path]);
        let context = context.lock_read();

        SkyboxFrontFaceDownscaler::new(&context)
            .expect("skybox front-face thumbnail shader should build");
    }
}
