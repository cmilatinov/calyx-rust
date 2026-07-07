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
use nalgebra_glm::{vec3, vec4, Vec3};
use sha1::{Digest, Sha1};
use uuid::Uuid;

mod cache;
mod camera_fit;
mod generator;
mod pipeline;
mod service;
#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use pipeline::{ThumbnailJob, ThumbnailPipeline, ThumbnailPriority, ThumbnailStatus};
pub use service::ThumbnailService;

const THUMBNAIL_MAX_FAILURES: u8 = 3;
pub const THUMBNAIL_DEFAULT_SIZE: u32 = 512;
pub const THUMBNAIL_MIN_SIZE: u32 = 64;
pub const THUMBNAIL_MAX_SIZE: u32 = 512;
pub const THUMBNAIL_DEFAULT_FRAME_MARGIN: f32 = 0.025;
pub const THUMBNAIL_MIN_FRAME_MARGIN: f32 = 0.0;
pub const THUMBNAIL_MAX_FRAME_MARGIN: f32 = 0.45;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThumbnailRenderSettings {
    pub size_px: u32,
    pub frame_margin: f32,
}

impl Default for ThumbnailRenderSettings {
    fn default() -> Self {
        Self {
            size_px: THUMBNAIL_DEFAULT_SIZE,
            frame_margin: THUMBNAIL_DEFAULT_FRAME_MARGIN,
        }
    }
}

impl ThumbnailRenderSettings {
    pub fn with_frame_margin(frame_margin: f32) -> Self {
        Self {
            frame_margin,
            ..Default::default()
        }
        .sanitized()
    }

    pub fn with_size_px(size_px: u32) -> Self {
        Self {
            size_px,
            ..Default::default()
        }
        .sanitized()
    }

    pub fn with_size_and_frame_margin(size_px: u32, frame_margin: f32) -> Self {
        Self {
            size_px,
            frame_margin,
        }
        .sanitized()
    }

    fn cache_key(self) -> String {
        let frame_margin_millis = (self.sanitized().frame_margin * 1000.0).round() as u32;
        format!("frame-margin-{frame_margin_millis}")
    }

    fn sanitized(self) -> Self {
        let size_px = self.size_px.clamp(THUMBNAIL_MIN_SIZE, THUMBNAIL_MAX_SIZE);
        let frame_margin = if self.frame_margin.is_finite() {
            self.frame_margin
                .clamp(THUMBNAIL_MIN_FRAME_MARGIN, THUMBNAIL_MAX_FRAME_MARGIN)
        } else {
            THUMBNAIL_DEFAULT_FRAME_MARGIN
        };
        Self {
            size_px,
            frame_margin,
        }
    }
}

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
