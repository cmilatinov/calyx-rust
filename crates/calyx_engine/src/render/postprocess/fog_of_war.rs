//! The fog of war pass: darkens what a side cannot see, by the ground
//! position of each pixel's surface, on display-linear color after tone
//! mapping.

use super::{
    CameraPlanes, NoPipeline, PassContext, PassInputs, PassIo, PostProcessPass,
    PostProcessSettings, PostProcessStage, SceneInput,
};
use crate::component::{ComponentFogOfWar, FogOfWarBlocker, FogOfWarSight};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::buffer::ResizableBuffer;
use crate::render::{CachedBindGroup, PipelineOptions, RenderUtils, SceneRendererError, Shader};
use egui_wgpu::wgpu;
use nalgebra_glm::Vec2;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct FogOfWarUniform {
    /// sRGB-encoded fog colour in rgb, opacity in a.
    color: [f32; 4],
    camera: CameraPlanes,
    view_to_world: [[f32; 4]; 4],
    viewer_count: u32,
    blocker_count: u32,
    /// [`SIGHT_SECTORS`], as the shader reads it.
    sector_count: u32,
    _pad: u32,
}

/// How many equal angular sectors around each viewer the pass sorts the
/// blockers into. A line of sight only meets the blockers of the sectors
/// its direction lies in, so each pixel tests those rather than every
/// blocker in the viewer's reach.
pub(crate) const SIGHT_SECTORS: usize = 64;

/// Angle, in radians, by which a blocker's angular span is widened, so
/// rounding never leaves a blocker out of a sector a line through it lies
/// in. The shader widens a pixel's window of directions by as much.
const SECTOR_SLACK: f32 = 1e-3;

/// One viewer as the shader reads it: `Viewer` in
/// `shaders/postprocess/fog_of_war.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GpuViewer {
    position: [f32; 2],
    fade_start: f32,
    fade_end: f32,
    fade_curve: f32,
    weight: f32,
    /// The viewer's first of [`SIGHT_SECTORS`] entries in the sector table.
    first_sector: u32,
    _pad: u32,
}

/// One blocker as the shader reads it: `Blocker` in
/// `shaders/postprocess/fog_of_war.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GpuBlocker {
    center: [f32; 2],
    local_x: [f32; 2],
    local_y: [f32; 2],
    scale: [f32; 2],
}

/// The tables the shader reads a sight from: its viewers and blockers, and
/// for each viewer which blockers lie in each angular sector around it.
#[derive(Debug, Default)]
pub(crate) struct SightTables {
    pub viewers: Vec<GpuViewer>,
    pub blockers: Vec<GpuBlocker>,
    /// [`SIGHT_SECTORS`] entries per viewer, from its `first_sector`: the
    /// first index into `sector_blockers` and how many follow.
    pub sectors: Vec<[u32; 2]>,
    /// Indices into `blockers`, sector by sector.
    pub sector_blockers: Vec<u32>,
    /// The blockers of each sector of the viewer being packed, reused.
    buckets: Vec<Vec<u32>>,
}

/// Packs `sight` into the shader's tables, reusing their storage. Flat
/// blockers, which block nothing, are left out.
///
/// Each viewer's surroundings split into [`SIGHT_SECTORS`] equal angular
/// sectors, and each sector lists the blockers whose angular span, seen
/// from the viewer, overlaps it; a blocker the viewer stands in or next to
/// spans every sector. A line of sight from the viewer meets a blocker only
/// in a direction within that span, so the shader, testing the blockers of
/// the sectors a pixel's lines of sight lie in, reaches the answer testing
/// every blocker would.
pub(crate) fn pack_sight(sight: &FogOfWarSight, tables: &mut SightTables) {
    tables.viewers.clear();
    tables.blockers.clear();
    tables.sectors.clear();
    tables.sector_blockers.clear();
    tables.buckets.resize_with(SIGHT_SECTORS, Vec::new);
    let mut kept = Vec::with_capacity(sight.blockers.len());
    for blocker in &sight.blockers {
        let Some([x, y]) = blocker.to_local() else {
            continue;
        };
        kept.push(blocker);
        tables.blockers.push(GpuBlocker {
            center: [blocker.center.x, blocker.center.y],
            local_x: [x.x, x.y],
            local_y: [y.x, y.y],
            scale: [x.norm(), y.norm()],
        });
    }
    for viewer in &sight.viewers {
        for bucket in &mut tables.buckets {
            bucket.clear();
        }
        for (index, blocker) in kept.iter().enumerate() {
            let (first, count) = blocker_sectors(blocker, viewer.position);
            for sector in first..first + count as i64 {
                tables.buckets[sector.rem_euclid(SIGHT_SECTORS as i64) as usize].push(index as u32);
            }
        }
        let first_sector = tables.sectors.len() as u32;
        for bucket in &tables.buckets {
            tables
                .sectors
                .push([tables.sector_blockers.len() as u32, bucket.len() as u32]);
            tables.sector_blockers.extend_from_slice(bucket);
        }
        tables.viewers.push(GpuViewer {
            position: [viewer.position.x, viewer.position.y],
            fade_start: viewer.fade_start,
            fade_end: viewer.fade_end,
            fade_curve: viewer.fade_curve,
            weight: viewer.weight,
            first_sector,
            _pad: 0,
        });
    }
}

/// The sectors around `viewer` that `blocker` spans, as the first one,
/// which may be negative or past the last and wraps around, and how many
/// follow: those overlapping the angles under which the circle around the
/// blocker's corners shows from the viewer. A viewer within that circle
/// sees the blocker in every direction.
fn blocker_sectors(blocker: &FogOfWarBlocker, viewer: Vec2) -> (i64, usize) {
    let offset = blocker.center - viewer;
    let distance = offset.norm();
    // Sight carries on past a blocker's side by up to the touching
    // distance.
    let radius = blocker.radius() + FogOfWarSight::TOUCHING;
    if distance <= radius * 1.001 {
        return (0, SIGHT_SECTORS);
    }
    let sector_angle = std::f32::consts::TAU / SIGHT_SECTORS as f32;
    let center = offset.y.atan2(offset.x).rem_euclid(std::f32::consts::TAU);
    let half_width = (radius / distance).asin() + SECTOR_SLACK;
    let first = ((center - half_width) / sector_angle).floor() as i64;
    let last = ((center + half_width) / sector_angle).floor() as i64;
    (first, ((last - first + 1) as usize).min(SIGHT_SECTORS))
}

/// Fog of war in the `Ldr` stage, after the ink outlines and before FXAA,
/// which smooths the image's other edges. It reads the scene depth,
/// rebuilds each pixel's world position, traces lines of sight from the
/// side's viewers past its blockers at a few points across the pixel's
/// ground footprint, and mixes the pixel towards the fog colour by how
/// little of it the side sees (see [`ComponentFogOfWar`] and
/// [`FogOfWarSight::pixel_visibility`]). It runs while the
/// settings hold a [`ComponentFogOfWar`] that game code gave a sight.
pub struct FogOfWarPass {
    shader: Ref<Shader>,
    uniform_buffer: wgpu::Buffer,
    viewer_buffer: ResizableBuffer,
    blocker_buffer: ResizableBuffer,
    sector_buffer: ResizableBuffer,
    sector_blocker_buffer: ResizableBuffer,
    tables: SightTables,
    /// Keyed by the layout, the input and depth views, and the generations
    /// of the viewer, blocker, sector and sector blocker buffers.
    #[allow(clippy::type_complexity)]
    bind_group: CachedBindGroup<(
        wgpu::BindGroupLayout,
        wgpu::TextureView,
        wgpu::TextureView,
        [u64; 4],
    )>,
}

impl FogOfWarPass {
    /// Shader asset the pass draws with.
    pub const SHADER: &'static str = "shaders/postprocess/fog_of_war";

    /// Creates the pass, or reports why its shader did not load.
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        let shader = SceneRendererError::load::<Shader>(context, Self::SHADER)?;
        let uniform_buffer =
            context
                .render_context
                .device()
                .create_buffer(&wgpu::BufferDescriptor {
                    label: Some("fog_of_war_uniform_buffer"),
                    size: size_of::<FogOfWarUniform>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
        let storage =
            || ResizableBuffer::new(wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
        Ok(Self {
            shader,
            uniform_buffer,
            viewer_buffer: storage(),
            blocker_buffer: storage(),
            sector_buffer: storage(),
            sector_blocker_buffer: storage(),
            tables: SightTables::default(),
            bind_group: Default::default(),
        })
    }
}

impl PostProcessPass for FogOfWarPass {
    fn name(&self) -> &str {
        "fog_of_war"
    }

    fn stage(&self) -> PostProcessStage {
        PostProcessStage::Ldr
    }

    /// The scene depth, to place each pixel's surface in the world.
    fn inputs(&self) -> PassInputs {
        SceneInput::Depth.into()
    }

    fn enabled(&self, settings: &PostProcessSettings) -> bool {
        settings
            .get::<ComponentFogOfWar>()
            .is_some_and(|fog| fog.sight.is_some())
    }

    fn prepare(&mut self, context: &PassContext, settings: &PostProcessSettings) {
        let Some(fog) = settings.get::<ComponentFogOfWar>() else {
            return;
        };
        let Some(sight) = &fog.sight else {
            return;
        };
        pack_sight(sight, &mut self.tables);
        let device = context.render_context.device();
        let queue = context.render_context.queue();
        // Storage bindings need a buffer of at least one element.
        self.viewer_buffer
            .resize(device, size_of::<GpuViewer>() as u64);
        self.viewer_buffer
            .write_buffer(device, queue, &self.tables.viewers, None);
        self.blocker_buffer
            .resize(device, size_of::<GpuBlocker>() as u64);
        self.blocker_buffer
            .write_buffer(device, queue, &self.tables.blockers, None);
        self.sector_buffer
            .resize(device, size_of::<[u32; 2]>() as u64);
        self.sector_buffer
            .write_buffer(device, queue, &self.tables.sectors, None);
        self.sector_blocker_buffer
            .resize(device, size_of::<u32>() as u64);
        self.sector_blocker_buffer
            .write_buffer(device, queue, &self.tables.sector_blockers, None);
        let uniform = FogOfWarUniform {
            color: [
                fog.color.r() as f32 / 255.0,
                fog.color.g() as f32 / 255.0,
                fog.color.b() as f32 / 255.0,
                fog.opacity,
            ],
            camera: context.camera,
            view_to_world: context.view_to_world.into(),
            viewer_count: self.tables.viewers.len() as u32,
            blocker_count: self.tables.blockers.len() as u32,
            sector_count: SIGHT_SECTORS as u32,
            _pad: 0,
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniform));
    }

    fn record(
        &mut self,
        context: &PassContext,
        encoder: &mut wgpu::CommandEncoder,
        io: PassIo,
    ) -> Result<(), NoPipeline> {
        let depth = io
            .depth
            .expect("the chain gives depth to a pass that asks for it");
        let shader = self.shader.read();
        let Some(pipeline) = shader.pipeline(
            &PipelineOptions::fullscreen_builder()
                .target(RenderUtils::color_default(io.output.descriptor.format))
                .build(),
        ) else {
            return Err(NoPipeline::of(&shader));
        };
        let layout = shader.bind_group_layouts[0].clone();
        drop(shader);
        let device = context.render_context.device();
        let uniform_buffer = &self.uniform_buffer;
        let viewer_buffer = self.viewer_buffer.get_wgpu_buffer();
        let blocker_buffer = self.blocker_buffer.get_wgpu_buffer();
        let sector_buffer = self.sector_buffer.get_wgpu_buffer();
        let sector_blocker_buffer = self.sector_blocker_buffer.get_wgpu_buffer();
        let bind_group = self.bind_group.get_or_create(
            (
                layout.clone(),
                io.input.view.clone(),
                depth.view.clone(),
                [
                    self.viewer_buffer.generation(),
                    self.blocker_buffer.generation(),
                    self.sector_buffer.generation(),
                    self.sector_blocker_buffer.generation(),
                ],
            ),
            || {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("fog_of_war_bind_group"),
                    layout: &layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&io.input.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&depth.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: uniform_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: viewer_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: blocker_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: sector_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 6,
                            resource: sector_blocker_buffer.as_entire_binding(),
                        },
                    ],
                })
            },
        );
        let mut render_pass = RenderUtils::fullscreen_pass(encoder, &io.output.view, "Fog of war");
        render_pass.set_pipeline(&pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        RenderUtils::draw_fullscreen(&mut render_pass);
        Ok(())
    }

    fn release_targets(&mut self) {
        self.bind_group.clear();
    }
}

#[cfg(test)]
mod tests;
