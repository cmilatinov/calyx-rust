//! Shadow maps for directional and spot lights.
//!
//! [`ShadowPlan`] chooses, each frame, which lights get a shadow and fits
//! their light-space projections to the camera: the first shadowed
//! directional light gets [`SHADOW_CASCADE_COUNT`] cascades, and up to
//! [`MAX_SPOT_SHADOWS`] shadowed spot lights, nearest the camera first, get a
//! tile of the spot atlas. [`ShadowRenderer`] renders the plan's depth maps
//! from the scene pass's draw list and owns what the lighting shaders bind
//! beside the lights, in `assets/shaders/common/shadows.wgsl`.
//!
//! `docs/design/rendering/shadows.md` describes the design.

use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::math::{self, Transform};
use crate::render::asset_render_state::LockedAssetRenderState;
use crate::render::{
    CachedBindGroup, Camera, DrawBatch, PipelineOptions, RenderUtils, SceneRendererError, Shader,
};
use egui_wgpu::wgpu;
use nalgebra_glm as glm;
use nalgebra_glm::{Mat4, Vec3};

/// How many cascades the shadowed directional light splits its distance into.
pub const SHADOW_CASCADE_COUNT: usize = 4;
/// Width and height in texels of each cascade.
pub const SHADOW_CASCADE_RESOLUTION: u32 = 2048;
/// How many spot lights can cast shadows at once.
pub const MAX_SPOT_SHADOWS: usize = 8;
/// Width and height in texels of the spot shadow atlas.
pub const SHADOW_ATLAS_RESOLUTION: u32 = 4096;
/// Width and height in texels of one spot light's atlas tile.
pub const SHADOW_ATLAS_TILE: u32 = 1024;
/// Tiles per atlas row.
const ATLAS_COLUMNS: u32 = SHADOW_ATLAS_RESOLUTION / SHADOW_ATLAS_TILE;
/// The blend of the practical split scheme: 0 splits the distance evenly,
/// 1 logarithmically.
const CASCADE_SPLIT_LAMBDA: f32 = 0.75;
/// The fraction of each cascade's depth range, at its far end, over which it
/// blends into the next.
const CASCADE_BLEND: f32 = 0.1;
/// Cascade radii round up to this step, in world units, so a cascade keeps
/// its size, and its texel size, as the camera turns.
const CASCADE_RADIUS_STEP: f32 = 1.0 / 16.0;
/// Constant depth bias, in units of the depth format, per unit of a light's
/// `depth_bias`.
const DEPTH_BIAS_CONSTANT: f32 = 1000.0;
/// Slope-scaled depth bias per unit of a light's `depth_bias`.
const DEPTH_BIAS_SLOPE: f32 = 2.0;
/// Extra half-angle, in degrees, a spot light's shadow frustum opens past its
/// outer cone, so filtering at the cone's edge reads inside the tile.
const SPOT_FOV_MARGIN: f32 = 2.0;
/// The widest full field of view, in degrees, of a spot light's shadow.
const SPOT_MAX_FOV: f32 = 170.0;
/// The shadow depth format.
const SHADOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// A shadowed light's shadow settings, from its component.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct ShadowSettings {
    /// For a directional light, the view depth its cascades cover; for a
    /// spot light, the distance from the camera within which it keeps its
    /// shadow.
    pub distance: f32,
    /// Scales the shadow pass's constant and slope-scaled depth bias.
    pub depth_bias: f32,
    /// The normal offset of a shadow lookup, in shadow texels.
    pub normal_bias: f32,
}

impl ShadowSettings {
    /// The depth bias the shadow pass's pipeline applies for these settings.
    fn depth_bias_state(&self) -> wgpu::DepthBiasState {
        let bias = self.depth_bias.max(0.0);
        wgpu::DepthBiasState {
            constant: (bias * DEPTH_BIAS_CONSTANT).round() as i32,
            slope_scale: bias * DEPTH_BIAS_SLOPE,
            clamp: 0.0,
        }
    }
}

/// A light that may cast a shadow, as [`ShadowPlan::new`] reads it.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum ShadowCaster {
    Directional {
        /// Unit world direction the light travels along.
        direction: Vec3,
        settings: ShadowSettings,
    },
    Spot {
        position: Vec3,
        /// Unit world direction the cone opens along.
        direction: Vec3,
        range: f32,
        /// The outer cone half-angle, in radians.
        outer_angle: f32,
        settings: ShadowSettings,
    },
}

/// The camera a frame's shadows are fitted to.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct ShadowCamera {
    /// Camera to world, without scale.
    world: Mat4,
    position: Vec3,
    /// Unit world axis the camera looks along.
    forward: Vec3,
    tan_half_fov_x: f32,
    tan_half_fov_y: f32,
    near: f32,
    far: f32,
}

impl ShadowCamera {
    pub fn new(camera: &Camera, transform: &Transform) -> Self {
        let world = glm::translation(&transform.position) * glm::quat_to_mat4(&transform.rotation);
        let forward = transform.rotation * Vec3::z();
        let fov_y = math::to_fov_y(camera.aspect, camera.fov_x);
        Self {
            world,
            position: transform.position,
            forward,
            tan_half_fov_x: (camera.fov_x * 0.5).tan(),
            tan_half_fov_y: (fov_y * 0.5).tan(),
            near: camera.near_plane.max(1e-3),
            far: camera.far_plane,
        }
    }

    /// The world corners of the frustum slice between view depths `near`
    /// and `far`.
    fn slice_corners(&self, near: f32, far: f32) -> [Vec3; 8] {
        let mut corners = [Vec3::zeros(); 8];
        for (i, corner) in corners.iter_mut().enumerate() {
            let depth = if i < 4 { near } else { far };
            let x = if i & 1 == 0 { -1.0 } else { 1.0 };
            let y = if i & 2 == 0 { -1.0 } else { 1.0 };
            let local = glm::vec4(
                x * depth * self.tan_half_fov_x,
                y * depth * self.tan_half_fov_y,
                depth,
                1.0,
            );
            *corner = (self.world * local).xyz();
        }
        corners
    }
}

/// The directional light's cascades.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CascadeShadow {
    /// World to each cascade's clip space.
    pub view_projection: [Mat4; SHADOW_CASCADE_COUNT],
    /// View depth at which each cascade ends.
    pub split_far: [f32; SHADOW_CASCADE_COUNT],
    /// World size of one texel of each cascade.
    pub texel_size: [f32; SHADOW_CASCADE_COUNT],
    pub settings: ShadowSettings,
}

/// One spot light's shadow.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SpotShadowView {
    /// World to the light's clip space.
    pub view_projection: Mat4,
    /// The atlas tile it renders into, also its `shadow_index`.
    pub tile: u32,
    /// World size of one texel per unit of distance from the light.
    pub texel_scale: f32,
    pub settings: ShadowSettings,
}

impl SpotShadowView {
    /// The tile's top left texel in the atlas.
    pub fn tile_origin(&self) -> (u32, u32) {
        tile_origin(self.tile)
    }
}

/// The top left texel of atlas tile `tile`, filled row by row.
pub(crate) fn tile_origin(tile: u32) -> (u32, u32) {
    (
        (tile % ATLAS_COLUMNS) * SHADOW_ATLAS_TILE,
        (tile / ATLAS_COLUMNS) * SHADOW_ATLAS_TILE,
    )
}

/// Which lights get a shadow this frame, and their light-space projections.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ShadowPlan {
    pub directional: Option<CascadeShadow>,
    pub spots: Vec<SpotShadowView>,
    pub camera_position: Vec3,
    pub camera_forward: Vec3,
}

impl ShadowPlan {
    /// Plans the shadows of `casters`, fitted to `camera`, and returns each
    /// caster's shadow index, -1 for none, in `casters` order. `None` in
    /// `casters` marks a light that casts no shadow; without a camera no
    /// light gets one.
    ///
    /// The first directional caster gets the cascades, index 0. Spot casters
    /// within their shadow distance of the camera get atlas tiles, nearest
    /// first, up to [`MAX_SPOT_SHADOWS`]; the tile is the index.
    pub fn new(
        casters: &[Option<ShadowCaster>],
        camera: Option<&ShadowCamera>,
    ) -> (Self, Vec<i32>) {
        let mut indices = vec![-1; casters.len()];
        let Some(camera) = camera else {
            return (Self::default(), indices);
        };
        let mut plan = Self {
            camera_position: camera.position,
            camera_forward: camera.forward,
            ..Default::default()
        };

        if let Some((index, direction, settings)) =
            casters
                .iter()
                .enumerate()
                .find_map(|(index, caster)| match caster {
                    Some(ShadowCaster::Directional {
                        direction,
                        settings,
                    }) => Some((index, *direction, *settings)),
                    _ => None,
                })
        {
            plan.directional = Some(fit_cascades(camera, direction, settings));
            indices[index] = 0;
        }

        let mut spots: Vec<(usize, f32)> = casters
            .iter()
            .enumerate()
            .filter_map(|(index, caster)| match caster {
                Some(ShadowCaster::Spot {
                    position, settings, ..
                }) => {
                    let distance = (position - camera.position).norm();
                    (distance <= settings.distance).then_some((index, distance))
                }
                _ => None,
            })
            .collect();
        spots.sort_by(|a, b| a.1.total_cmp(&b.1));
        for (tile, (index, _)) in spots.into_iter().take(MAX_SPOT_SHADOWS).enumerate() {
            let Some(ShadowCaster::Spot {
                position,
                direction,
                range,
                outer_angle,
                settings,
            }) = casters[index]
            else {
                unreachable!("only spot casters were kept");
            };
            plan.spots.push(fit_spot(
                tile as u32,
                position,
                direction,
                range,
                outer_angle,
                settings,
            ));
            indices[index] = tile as i32;
        }
        (plan, indices)
    }

    /// Whether any light casts a shadow this frame.
    pub fn is_empty(&self) -> bool {
        self.directional.is_none() && self.spots.is_empty()
    }
}

/// The view depths at which each cascade ends, between `near` and `far`, by
/// the practical split scheme: a blend of even and logarithmic splits.
pub(crate) fn cascade_splits(near: f32, far: f32) -> [f32; SHADOW_CASCADE_COUNT] {
    let far = far.max(near * 1.001);
    std::array::from_fn(|i| {
        let fraction = (i + 1) as f32 / SHADOW_CASCADE_COUNT as f32;
        let logarithmic = near * (far / near).powf(fraction);
        let even = near + (far - near) * fraction;
        CASCADE_SPLIT_LAMBDA * logarithmic + (1.0 - CASCADE_SPLIT_LAMBDA) * even
    })
}

/// The view depth from which cascade `index` is fitted: the camera's `near`
/// plane for the first, and for the others where the previous cascade starts
/// blending into it. `directional_shadow` in `shaders/common/shadows.wgsl`
/// blends over the last [`CASCADE_BLEND`] of a cascade's range, measured
/// from the split before it (0 for the first cascade), so the band reads both
/// maps.
fn cascade_fit_start(near: f32, splits: &[f32; SHADOW_CASCADE_COUNT], index: usize) -> f32 {
    if index == 0 {
        return near;
    }
    let far = splits[index - 1];
    let previous = if index >= 2 { splits[index - 2] } else { 0.0 };
    (far - (far - previous) * CASCADE_BLEND).max(near)
}

/// An up axis that is never parallel to `direction`.
fn up_for(direction: &Vec3) -> Vec3 {
    if direction.y.abs() > 0.99 {
        Vec3::z()
    } else {
        Vec3::y()
    }
}

fn fit_cascades(camera: &ShadowCamera, direction: Vec3, settings: ShadowSettings) -> CascadeShadow {
    let direction = direction.try_normalize(f32::EPSILON).unwrap_or(-Vec3::y());
    let far = settings.distance.max(camera.near * 2.0).min(camera.far);
    let split_far = cascade_splits(camera.near, far);
    // The light's rotation, about the world origin: snapping in this space
    // moves a cascade only in whole texels.
    let light_view = glm::look_at_lh(&Vec3::zeros(), &direction, &up_for(&direction));
    let mut view_projection = [Mat4::identity(); SHADOW_CASCADE_COUNT];
    let mut texel_size = [0.0; SHADOW_CASCADE_COUNT];
    for i in 0..SHADOW_CASCADE_COUNT {
        let corners =
            camera.slice_corners(cascade_fit_start(camera.near, &split_far, i), split_far[i]);
        let center = corners.iter().sum::<Vec3>() / corners.len() as f32;
        let radius = corners
            .iter()
            .map(|corner| (corner - center).norm())
            .fold(0.0f32, f32::max);
        // The slice's shape is fixed relative to the camera, so its bounding
        // sphere's radius is too; rounding hides float noise.
        let radius = ((radius / CASCADE_RADIUS_STEP).ceil() * CASCADE_RADIUS_STEP).max(0.01);
        // The map spans the sphere plus a texel each side, which snapping
        // the center to a texel may take up.
        let texel = 2.0 * radius / (SHADOW_CASCADE_RESOLUTION - 2) as f32;
        let half = radius + texel;
        let center = light_view * glm::vec4(center.x, center.y, center.z, 1.0);
        let x = (center.x / texel).floor() * texel;
        let y = (center.y / texel).floor() * texel;
        // Casters up to the shadow distance towards the light, outside the
        // sphere, still draw into the cascade.
        let projection = glm::ortho_lh_zo(
            x - half,
            x + half,
            y - half,
            y + half,
            center.z - radius - far,
            center.z + radius,
        );
        view_projection[i] = projection * light_view;
        texel_size[i] = texel;
    }
    CascadeShadow {
        view_projection,
        split_far,
        texel_size,
        settings,
    }
}

fn fit_spot(
    tile: u32,
    position: Vec3,
    direction: Vec3,
    range: f32,
    outer_angle: f32,
    settings: ShadowSettings,
) -> SpotShadowView {
    let direction = direction.try_normalize(f32::EPSILON).unwrap_or(Vec3::z());
    let fov =
        (2.0 * outer_angle + 2.0 * SPOT_FOV_MARGIN.to_radians()).min(SPOT_MAX_FOV.to_radians());
    let far = range.max(0.1);
    let near = (far * 0.01).clamp(0.02, 0.5);
    let view = glm::look_at_lh(&position, &(position + direction), &up_for(&direction));
    let projection = glm::perspective_lh_zo(1.0, fov, near, far);
    SpotShadowView {
        view_projection: projection * view,
        tile,
        texel_scale: 2.0 * (fov * 0.5).tan() / SHADOW_ATLAS_TILE as f32,
        settings,
    }
}

/// One cascade as `ShadowCascade` in `assets/shaders/common/shadows.wgsl`
/// reads it.
#[repr(C)]
#[derive(Default, Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuShadowCascade {
    view_projection: [[f32; 4]; 4],
    split_far: f32,
    texel_size: f32,
    _padding: [f32; 2],
}

/// One spot shadow as `SpotShadow` in `assets/shaders/common/shadows.wgsl`
/// reads it.
#[repr(C)]
#[derive(Default, Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuSpotShadow {
    view_projection: [[f32; 4]; 4],
    tile_offset: [f32; 2],
    texel_scale: f32,
    normal_bias: f32,
}

/// `ShadowData` in `assets/shaders/common/shadows.wgsl`.
#[repr(C)]
#[derive(Default, Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuShadowData {
    cascades: [GpuShadowCascade; SHADOW_CASCADE_COUNT],
    spots: [GpuSpotShadow; MAX_SPOT_SHADOWS],
    camera_position: [f32; 3],
    cascade_blend: f32,
    camera_forward: [f32; 3],
    directional_normal_bias: f32,
    atlas_tile_size: f32,
    atlas_texel: f32,
    cascade_texel: f32,
    _padding: f32,
}

impl From<&ShadowPlan> for GpuShadowData {
    fn from(plan: &ShadowPlan) -> Self {
        let mut data = GpuShadowData {
            camera_position: plan.camera_position.into(),
            cascade_blend: CASCADE_BLEND,
            camera_forward: plan.camera_forward.into(),
            atlas_tile_size: SHADOW_ATLAS_TILE as f32 / SHADOW_ATLAS_RESOLUTION as f32,
            atlas_texel: 1.0 / SHADOW_ATLAS_RESOLUTION as f32,
            cascade_texel: 1.0 / SHADOW_CASCADE_RESOLUTION as f32,
            ..Default::default()
        };
        if let Some(cascades) = &plan.directional {
            data.directional_normal_bias = cascades.settings.normal_bias.max(0.0);
            for (i, cascade) in data.cascades.iter_mut().enumerate() {
                *cascade = GpuShadowCascade {
                    view_projection: cascades.view_projection[i].into(),
                    split_far: cascades.split_far[i],
                    texel_size: cascades.texel_size[i],
                    _padding: [0.0; 2],
                };
            }
        }
        for spot in &plan.spots {
            let (x, y) = spot.tile_origin();
            data.spots[spot.tile as usize] = GpuSpotShadow {
                view_projection: spot.view_projection.into(),
                tile_offset: [
                    x as f32 / SHADOW_ATLAS_RESOLUTION as f32,
                    y as f32 / SHADOW_ATLAS_RESOLUTION as f32,
                ],
                texel_scale: spot.texel_scale,
                normal_bias: spot.settings.normal_bias.max(0.0),
            };
        }
        data
    }
}

/// A depth texture shadows render into, with the views the passes and the
/// lighting bind.
struct ShadowTexture {
    _texture: wgpu::Texture,
    /// The view the lighting shaders sample.
    sample_view: wgpu::TextureView,
    /// One render-attachment view per layer.
    layer_views: Vec<wgpu::TextureView>,
}

impl ShadowTexture {
    fn new(device: &wgpu::Device, label: &str, size: u32, layers: u32, array: bool) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SHADOW_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let sample_view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some(label),
            dimension: Some(if array {
                wgpu::TextureViewDimension::D2Array
            } else {
                wgpu::TextureViewDimension::D2
            }),
            ..Default::default()
        });
        let layer_views = (0..layers)
            .map(|layer| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some(label),
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: layer,
                    array_layer_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        Self {
            _texture: texture,
            sample_view,
            layer_views,
        }
    }
}

/// What the last [`ShadowRenderer::render`] drew, for tests and diagnostics.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ShadowFrameStats {
    /// Cascades rendered for the directional light.
    pub cascades: usize,
    /// Spot light tiles rendered into the atlas.
    pub spot_tiles: usize,
    /// Mesh instances drawn, summed over every cascade and tile.
    pub caster_instances: u32,
}

/// The shadow resources the lighting shaders bind in group 2, after the
/// light storage buffer.
pub(crate) struct ShadowBindings<'a> {
    pub data: &'a wgpu::Buffer,
    pub cascades: &'a wgpu::TextureView,
    pub atlas: &'a wgpu::TextureView,
    pub sampler: &'a wgpu::Sampler,
}

/// Renders a frame's [`ShadowPlan`] into the cascade array and the spot
/// atlas, and owns what the lighting shaders bind to read them. Each scene
/// renderer has its own, since cascades follow its camera.
pub(crate) struct ShadowRenderer {
    shader: Ref<Shader>,
    data_buffer: wgpu::Buffer,
    /// One light-space matrix per cascade, then per atlas tile.
    view_buffers: Vec<wgpu::Buffer>,
    view_bind_groups: Vec<CachedBindGroup<(wgpu::BindGroupLayout, wgpu::Buffer)>>,
    /// Allocated only while the directional light casts a shadow.
    cascades: Option<ShadowTexture>,
    /// Allocated only while a spot light casts a shadow.
    atlas: Option<ShadowTexture>,
    /// 1x1 stand-ins bound while a map is not allocated; nothing samples
    /// them, since no light then has a shadow index into them.
    fallback_cascades: ShadowTexture,
    fallback_atlas: ShadowTexture,
    sampler: wgpu::Sampler,
    stats: ShadowFrameStats,
}

impl ShadowRenderer {
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        let device = context.render_context.device();
        let shader = SceneRendererError::load::<Shader>(context, "shaders/mesh/shadow")?;
        let uniform = |label: &str, size: usize| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let views = SHADOW_CASCADE_COUNT + MAX_SPOT_SHADOWS;
        Ok(Self {
            shader,
            data_buffer: uniform("shadow_data", size_of::<GpuShadowData>()),
            view_buffers: (0..views)
                .map(|_| uniform("shadow_view", size_of::<[[f32; 4]; 4]>()))
                .collect(),
            view_bind_groups: (0..views).map(|_| Default::default()).collect(),
            cascades: None,
            atlas: None,
            fallback_cascades: ShadowTexture::new(device, "shadow_cascades_fallback", 1, 1, true),
            fallback_atlas: ShadowTexture::new(device, "shadow_atlas_fallback", 1, 1, false),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("shadow_sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Nearest,
                compare: Some(wgpu::CompareFunction::LessEqual),
                ..Default::default()
            }),
            stats: Default::default(),
        })
    }

    /// What the last frame drew.
    pub fn stats(&self) -> ShadowFrameStats {
        self.stats
    }

    /// Whether the cascade array and the spot atlas are allocated.
    pub fn allocated(&self) -> (bool, bool) {
        (self.cascades.is_some(), self.atlas.is_some())
    }

    /// What the lighting shaders bind to read this frame's shadows.
    pub fn bindings(&self) -> ShadowBindings<'_> {
        ShadowBindings {
            data: &self.data_buffer,
            cascades: &self
                .cascades
                .as_ref()
                .unwrap_or(&self.fallback_cascades)
                .sample_view,
            atlas: &self
                .atlas
                .as_ref()
                .unwrap_or(&self.fallback_atlas)
                .sample_view,
            sampler: &self.sampler,
        }
    }

    /// Uploads `plan` and renders its shadow maps, before the scene pass that
    /// samples them. Each map draws every batch of `draw_list` that casts
    /// shadows, the scene pass's own list, so a mesh the scene skips casts
    /// no shadow. A map no light needs is released.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        plan: &ShadowPlan,
        assets: &LockedAssetRenderState,
        draw_list: &[DrawBatch],
    ) {
        self.stats = Default::default();
        queue.write_buffer(
            &self.data_buffer,
            0,
            bytemuck::cast_slice(&[GpuShadowData::from(plan)]),
        );
        if plan.directional.is_none() {
            self.cascades = None;
        }
        if plan.spots.is_empty() {
            self.atlas = None;
        }
        if plan.is_empty() {
            return;
        }

        let shader = self.shader.read();
        let layout = shader.bind_group_layouts[0].clone();
        let mut view_bind_group = |index: usize, view_projection: &Mat4| {
            let matrix: [[f32; 4]; 4] = (*view_projection).into();
            queue.write_buffer(
                &self.view_buffers[index],
                0,
                bytemuck::cast_slice(&[matrix]),
            );
            let buffer = &self.view_buffers[index];
            self.view_bind_groups[index]
                .get_or_create((layout.clone(), buffer.clone()), || {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("shadow_view_bind_group"),
                        layout: &layout,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: buffer.as_entire_binding(),
                        }],
                    })
                })
                .clone()
        };

        if let Some(cascade_plan) = &plan.directional {
            let cascades = self.cascades.get_or_insert_with(|| {
                log::info!(
                    "Allocating {SHADOW_CASCADE_COUNT} shadow cascades of \
                     {SHADOW_CASCADE_RESOLUTION}x{SHADOW_CASCADE_RESOLUTION}"
                );
                ShadowTexture::new(
                    device,
                    "shadow_cascades",
                    SHADOW_CASCADE_RESOLUTION,
                    SHADOW_CASCADE_COUNT as u32,
                    true,
                )
            });
            let pipeline = shader.pipeline(&Self::pipeline_options(&cascade_plan.settings));
            for (i, view_projection) in cascade_plan.view_projection.iter().enumerate() {
                let bind_group = view_bind_group(i, view_projection);
                let mut pass =
                    Self::begin_pass(encoder, &cascades.layer_views[i], "Shadow Cascade");
                if let Some(pipeline) = &pipeline {
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, &bind_group, &[]);
                    self.stats.caster_instances += Self::draw_casters(&mut pass, assets, draw_list);
                }
                self.stats.cascades += 1;
            }
        }

        if !plan.spots.is_empty() {
            let atlas = self.atlas.get_or_insert_with(|| {
                log::info!(
                    "Allocating the {SHADOW_ATLAS_RESOLUTION}x{SHADOW_ATLAS_RESOLUTION} spot \
                     shadow atlas"
                );
                ShadowTexture::new(device, "shadow_atlas", SHADOW_ATLAS_RESOLUTION, 1, false)
            });
            let bind_groups: Vec<_> = plan
                .spots
                .iter()
                .map(|spot| {
                    view_bind_group(
                        SHADOW_CASCADE_COUNT + spot.tile as usize,
                        &spot.view_projection,
                    )
                })
                .collect();
            let mut pass = Self::begin_pass(encoder, &atlas.layer_views[0], "Spot Shadow Atlas");
            for (spot, bind_group) in plan.spots.iter().zip(&bind_groups) {
                let Some(pipeline) = shader.pipeline(&Self::pipeline_options(&spot.settings))
                else {
                    continue;
                };
                let (x, y) = spot.tile_origin();
                let tile = SHADOW_ATLAS_TILE;
                pass.set_viewport(x as f32, y as f32, tile as f32, tile as f32, 0.0, 1.0);
                pass.set_scissor_rect(x, y, tile, tile);
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, bind_group, &[]);
                self.stats.caster_instances += Self::draw_casters(&mut pass, assets, draw_list);
                self.stats.spot_tiles += 1;
            }
        }
    }

    /// The depth-only pipeline of a light with `settings`: single-sample, no
    /// color, both faces drawn so open meshes such as planes still cast, and
    /// the light's depth bias.
    fn pipeline_options(settings: &ShadowSettings) -> PipelineOptions {
        PipelineOptions::builder()
            .samples(1)
            .cull_mode(None)
            .fragment_targets(vec![])
            .depth_stencil(Some(wgpu::DepthStencilState {
                bias: settings.depth_bias_state(),
                ..RenderUtils::depth_default(SHADOW_FORMAT)
            }))
            .build()
    }

    fn begin_pass<'e>(
        encoder: &'e mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        label: &str,
    ) -> wgpu::RenderPass<'e> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[],
            depth_stencil_attachment: Some(RenderUtils::depth_stencil_attachment(view, 1.0, None)),
            timestamp_writes: None,
            occlusion_query_set: None,
        })
    }

    /// Draws every shadow-casting batch of `draw_list`, and returns how many
    /// instances it drew.
    fn draw_casters(
        pass: &mut wgpu::RenderPass<'_>,
        assets: &LockedAssetRenderState,
        draw_list: &[DrawBatch],
    ) -> u32 {
        let mut drawn = 0;
        for batch in draw_list.iter().filter(|batch| batch.cast_shadows) {
            let Some(mesh) = assets.mesh(batch.mesh_id) else {
                continue;
            };
            let Some(instance_group) = mesh.instance_bind_group() else {
                continue;
            };
            pass.set_bind_group(1, instance_group, &[]);
            RenderUtils::bind_mesh_buffers(pass, mesh);
            RenderUtils::draw_mesh_instanced(pass, mesh, batch.instances.clone());
            drawn += batch.instances.len() as u32;
        }
        drawn
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::UnitQuaternion;
    use nalgebra_glm::{vec3, vec4};

    fn settings(distance: f32) -> ShadowSettings {
        ShadowSettings {
            distance,
            depth_bias: 1.0,
            normal_bias: 1.0,
        }
    }

    fn camera_at(position: Vec3, forward: Vec3) -> ShadowCamera {
        let camera = Camera::new(16.0 / 9.0, 70f32.to_radians(), 0.1, 1000.0);
        let transform = Transform::from_components(
            position,
            UnitQuaternion::face_towards(&forward, &Vec3::y()),
            vec3(1.0, 1.0, 1.0),
        );
        ShadowCamera::new(&camera, &transform)
    }

    fn spot(position: Vec3, distance: f32) -> Option<ShadowCaster> {
        Some(ShadowCaster::Spot {
            position,
            direction: -Vec3::y(),
            range: 10.0,
            outer_angle: 30f32.to_radians(),
            settings: settings(distance),
        })
    }

    fn sun(distance: f32) -> Option<ShadowCaster> {
        Some(ShadowCaster::Directional {
            direction: vec3(1.0, -2.0, 0.5).normalize(),
            settings: settings(distance),
        })
    }

    fn clip(view_projection: &Mat4, point: Vec3) -> Vec3 {
        let clip = view_projection * vec4(point.x, point.y, point.z, 1.0);
        clip.xyz() / clip.w
    }

    #[test]
    fn splits_blend_even_and_logarithmic_and_end_at_the_shadow_distance() {
        let splits = cascade_splits(0.1, 50.0);
        assert!((splits[SHADOW_CASCADE_COUNT - 1] - 50.0).abs() < 1e-3);
        assert!(splits.windows(2).all(|pair| pair[0] < pair[1]));
        for (i, split) in splits.iter().enumerate() {
            let fraction = (i + 1) as f32 / SHADOW_CASCADE_COUNT as f32;
            let logarithmic = 0.1 * (50.0f32 / 0.1).powf(fraction);
            let even = 0.1 + 49.9 * fraction;
            assert!(
                *split > logarithmic.min(even) - 1e-4 && *split < logarithmic.max(even) + 1e-4,
                "split {i} lies between the even and logarithmic splits: {split}"
            );
        }
    }

    #[test]
    fn the_first_shadowed_directional_light_gets_the_cascades() {
        let camera = camera_at(Vec3::zeros(), Vec3::z());
        let (plan, indices) = ShadowPlan::new(&[None, sun(40.0), sun(20.0)], Some(&camera));
        assert_eq!(indices, [-1, 0, -1]);
        let cascades = plan.directional.expect("the first sun has cascades");
        assert_eq!(cascades.settings.distance, 40.0);
        assert!((cascades.split_far[SHADOW_CASCADE_COUNT - 1] - 40.0).abs() < 1e-3);
    }

    #[test]
    fn without_a_camera_no_light_gets_a_shadow() {
        let (plan, indices) = ShadowPlan::new(&[sun(40.0), spot(Vec3::zeros(), 50.0)], None);
        assert!(plan.is_empty());
        assert_eq!(indices, [-1, -1]);
    }

    #[test]
    fn each_cascade_holds_its_slice_and_the_blend_band_before_it() {
        // `directional_shadow` blends a point over the last `CASCADE_BLEND`
        // of cascade i - 1, measured from the split before that (0 for the
        // first), into cascade i, so cascade i must hold that band too. A
        // narrow field of view makes the slices short and wide, where a fit
        // from the split alone misses part of the band.
        let narrow = {
            let camera = Camera::new(16.0 / 9.0, 10f32.to_radians(), 0.1, 1000.0);
            let transform = Transform::from_components(
                Vec3::zeros(),
                UnitQuaternion::face_towards(&Vec3::z(), &Vec3::y()),
                vec3(1.0, 1.0, 1.0),
            );
            ShadowCamera::new(&camera, &transform)
        };
        let wide = camera_at(vec3(3.0, 2.0, -4.0), vec3(0.3, -0.2, 1.0).normalize());
        let side_sun = Some(ShadowCaster::Directional {
            direction: Vec3::x(),
            settings: settings(50.0),
        });
        for (name, camera, caster) in [
            ("wide", wide, sun(30.0)),
            ("narrow", narrow, sun(50.0)),
            ("narrow, side light", narrow, side_sun),
        ] {
            let (plan, _) = ShadowPlan::new(&[caster], Some(&camera));
            let cascades = plan.directional.unwrap();
            let splits = cascades.split_far;
            for i in 0..SHADOW_CASCADE_COUNT {
                let start = if i == 0 {
                    camera.near
                } else {
                    let before = if i >= 2 { splits[i - 2] } else { 0.0 };
                    splits[i - 1] - (splits[i - 1] - before) * CASCADE_BLEND
                };
                for corner in camera.slice_corners(start, splits[i]) {
                    let ndc = clip(&cascades.view_projection[i], corner);
                    assert!(
                        ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0 && (0.0..=1.0).contains(&ndc.z),
                        "{name}: cascade {i} misses corner {corner} of [{start}, {}]: {ndc}",
                        splits[i]
                    );
                }
            }
        }
    }

    #[test]
    fn a_moving_camera_moves_cascades_by_whole_texels_and_keeps_their_size() {
        let forward = vec3(0.2, -0.3, 1.0).normalize();
        let (before, _) = ShadowPlan::new(&[sun(30.0)], Some(&camera_at(Vec3::zeros(), forward)));
        let (after, _) = ShadowPlan::new(
            &[sun(30.0)],
            Some(&camera_at(vec3(0.137, 0.0, 0.291), forward)),
        );
        let (before, after) = (before.directional.unwrap(), after.directional.unwrap());
        for i in 0..SHADOW_CASCADE_COUNT {
            assert_eq!(before.texel_size[i], after.texel_size[i]);
            // A fixed world point moves across the map by a whole number of
            // texels, so the rasterized shadow does not shimmer.
            let point = vec3(1.0, 0.5, 2.0);
            let shift = (clip(&after.view_projection[i], point)
                - clip(&before.view_projection[i], point))
            .xy()
                * (SHADOW_CASCADE_RESOLUTION as f32 / 2.0);
            for axis in [shift.x, shift.y] {
                assert!(
                    (axis - axis.round()).abs() < 1e-2,
                    "cascade {i} moved by a fraction of a texel: {shift}"
                );
            }
        }
        // Turning the camera keeps each cascade's size.
        let (turned, _) = ShadowPlan::new(&[sun(30.0)], Some(&camera_at(Vec3::zeros(), Vec3::x())));
        assert_eq!(turned.directional.unwrap().texel_size, before.texel_size);
    }

    #[test]
    fn the_nearest_spot_lights_within_their_distance_get_tiles() {
        let camera = camera_at(Vec3::zeros(), Vec3::z());
        let mut casters: Vec<Option<ShadowCaster>> = (0..10)
            .map(|i| spot(vec3(0.0, 0.0, 10.0 - i as f32), 100.0))
            .collect();
        // Past its shadow distance, so it gets no tile however near it is.
        casters.push(spot(vec3(0.0, 0.0, 0.5), 0.1));
        casters.push(None);
        let (plan, indices) = ShadowPlan::new(&casters, Some(&camera));
        assert_eq!(plan.spots.len(), MAX_SPOT_SHADOWS);
        // Nearest first: the last spot created is the nearest.
        assert_eq!(indices[9], 0);
        assert_eq!(indices[2], 7);
        assert_eq!(
            &indices[0..2],
            [-1, -1],
            "the two farthest lose their shadow"
        );
        assert_eq!(&indices[10..], [-1, -1]);
        let tiles: Vec<u32> = plan.spots.iter().map(|spot| spot.tile).collect();
        assert_eq!(tiles, (0..MAX_SPOT_SHADOWS as u32).collect::<Vec<_>>());
    }

    #[test]
    fn tiles_fill_the_atlas_row_by_row_without_overlapping() {
        assert_eq!(tile_origin(0), (0, 0));
        assert_eq!(tile_origin(1), (SHADOW_ATLAS_TILE, 0));
        assert_eq!(tile_origin(ATLAS_COLUMNS), (0, SHADOW_ATLAS_TILE));
        for tile in 0..MAX_SPOT_SHADOWS as u32 {
            let (x, y) = tile_origin(tile);
            assert!(x + SHADOW_ATLAS_TILE <= SHADOW_ATLAS_RESOLUTION);
            assert!(y + SHADOW_ATLAS_TILE <= SHADOW_ATLAS_RESOLUTION);
        }
    }

    #[test]
    fn a_spot_shadow_covers_its_cone_out_to_its_range() {
        let position = vec3(1.0, 4.0, 2.0);
        let view = fit_spot(
            3,
            position,
            -Vec3::y(),
            10.0,
            30f32.to_radians(),
            settings(50.0),
        );
        let edge = 29f32.to_radians();
        let in_cone = position + vec3(edge.sin(), -edge.cos(), 0.0) * 9.0;
        let ndc = clip(&view.view_projection, in_cone);
        assert!(ndc.x.abs() < 1.0 && ndc.y.abs() < 1.0 && (0.0..1.0).contains(&ndc.z));
        let past_range = clip(&view.view_projection, position - Vec3::y() * 11.0);
        assert!(past_range.z > 1.0);
    }

    #[test]
    fn the_shadow_data_matches_the_shader_layout() {
        assert_eq!(size_of::<GpuShadowCascade>(), 80);
        assert_eq!(size_of::<GpuSpotShadow>(), 80);
        assert_eq!(std::mem::offset_of!(GpuShadowData, spots), 320);
        assert_eq!(std::mem::offset_of!(GpuShadowData, camera_position), 960);
        assert_eq!(std::mem::offset_of!(GpuShadowData, camera_forward), 976);
        assert_eq!(std::mem::offset_of!(GpuShadowData, atlas_tile_size), 992);
        assert_eq!(size_of::<GpuShadowData>(), 1008);
    }

    #[test]
    fn depth_bias_scales_with_the_light_setting() {
        assert_eq!(
            settings(1.0).depth_bias_state().slope_scale,
            DEPTH_BIAS_SLOPE
        );
        let none = ShadowSettings {
            depth_bias: 0.0,
            ..settings(1.0)
        };
        assert_eq!(none.depth_bias_state(), wgpu::DepthBiasState::default());
    }
}
