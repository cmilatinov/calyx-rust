//! Track marks: moving tanks print their two treads into the ground, and the
//! prints fade away.
//!
//! [`ComponentTrackMarks`] sits on the terrain, which tanks give it when play
//! starts (see [`attach`]), so any map with a terrain gets marks. Each frame
//! it looks at every tank, and for each one that moved on the terrain it
//! stamps both treads' contact patches into a [`MarksField`]: one byte of
//! mark strength per texel over the terrain's rectangle. Prints only add, so
//! a tank driving over its own marks keeps them fresh, and turning in place
//! sweeps the treads round into scuffs.
//!
//! The field fades by tiles: every [`FADE_STEP`] seconds an active tile takes
//! off the strength that time is worth, so a full-strength print is gone after
//! [`ComponentTrackMarks::fade_time`]. Only the tiles stamped or faded this
//! frame are written to the GPU, and a tile left with no marks drops out, so
//! the cost follows how much of the ground holds fresh marks, never how far
//! tanks have driven: there are no per-mark objects.
//!
//! The field reaches the screen through the ground material's marks hook
//! (`marks_texture` over `marks_rect`, see the `ground` shader): the
//! component draws the terrain with a copy of its material that samples the
//! field's texture, so the marks show only on the ground, under fog, light
//! and SSAO like the rest of it, and never on bridges, water or props. A
//! tank fog of war hides leaves no marks, so the ground never gives an unseen
//! enemy away.

use crate::combat_vfx::{CombatPalette, ComponentCombatPalette};
use crate::tank::ComponentTankController;
use calyx_engine::assets::material::{Material, MaterialTexture, ShaderVariableValue};
use calyx_engine::assets::texture::Texture;
use calyx_engine::assets::AssetRef;
use calyx_engine::component::{
    Component, ComponentEventContext, ComponentRef, ComponentTerrain, ComponentUpdate,
    ReflectComponent, ReflectComponentUpdate,
};
use calyx_engine::context::ReadOnlyRegistryContext;
use calyx_engine::core::Ref;
use calyx_engine::input::Input;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::resource::ResourceMap;
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid, Uuid};
use nalgebra_glm::{vec2, Vec2, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Texels per side of a fade and upload tile.
const TILE: usize = 32;
/// How often an active tile fades, in seconds. Each step takes off what the
/// time since the last one is worth, so the fade rate does not depend on it.
pub(crate) const FADE_STEP: f32 = 0.25;
/// The largest texture side the field takes; a larger terrain gets fewer
/// texels per metre.
const MAX_TEXELS: usize = 4096;

/// Where a tread runs: its centre line's distance from the hull's centre
/// line, in metres.
const TREAD_OFFSET: f32 = 0.8;
/// The width of the band one tread prints, in metres.
const TREAD_WIDTH: f32 = 0.45;
/// The length of a tread's ground contact, in metres.
const TREAD_LENGTH: f32 = 2.4;
/// The spacing of the grousers across a print, in metres, and how much
/// weaker the print is between them.
const GROUSER_PITCH: f32 = 0.4;
const GROUSER_DEPTH: f32 = 0.25;
/// Farther than this in one frame is a respawn, not driving: no print.
const MAX_STEP: f32 = 3.0;
/// How far above the terrain the hull may ride and still print, in metres;
/// higher, it is on a bridge, ramp or deck.
const GROUND_TOLERANCE: f32 = 0.25;
/// Smaller moves than these leave the last print as it is.
const MIN_MOVE: f32 = 0.005;
const MIN_TURN: f32 = 0.002;

/// Prints moving tanks leave in the terrain it sits on; see the module docs.
/// Tanks add it to the scene's terrain when play starts if no object has it,
/// with its defaults; give the terrain one to tune it.
#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "6a3e9c1d-5b27-4f80-8d41-c2f7a9e05b63"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Track Marks")]
#[serde(default)]
pub struct ComponentTrackMarks {
    /// Seconds a fresh, full-strength print takes to fade away.
    #[reflect_attr(min = 0.0, speed = 0.5)]
    pub fade_time: f32,
    /// How dark a fresh print is, as a share of the palette's track tint.
    #[reflect_attr(min = 0.0, max = 1.0, speed = 0.01)]
    pub strength: f32,
    /// Resolution of the marks, in texels per metre of terrain.
    #[reflect_attr(min = 1.0, speed = 0.5)]
    pub texels_per_metre: f32,
    #[serde(skip)]
    #[reflect_skip]
    runtime: Option<Box<MarksRuntime>>,
    /// Set once the terrain turned out unable to take marks, so it is not
    /// tried again every frame.
    #[serde(skip)]
    #[reflect_skip]
    unavailable: bool,
}

impl Default for ComponentTrackMarks {
    fn default() -> Self {
        Self {
            fade_time: 25.0,
            strength: 1.0,
            texels_per_metre: 8.0,
            runtime: None,
            unavailable: false,
        }
    }
}

impl Component for ComponentTrackMarks {}

impl ComponentUpdate for ComponentTrackMarks {
    fn update(
        this: ComponentRef<Self>,
        ComponentEventContext {
            scene, registries, ..
        }: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        let terrain = this.game_object();
        let Some((fade_time, strength, texels_per_metre, runtime, unavailable)) =
            this.write(scene, |marks| {
                (
                    marks.fade_time,
                    marks.strength,
                    marks.texels_per_metre,
                    marks.runtime.take(),
                    marks.unavailable,
                )
            })
        else {
            return;
        };
        if unavailable {
            return;
        }
        let runtime = runtime.or_else(|| {
            MarksRuntime::create(scene, terrain, registries, texels_per_metre).map(Box::new)
        });
        let Some(mut runtime) = runtime else {
            let _ = this.write(scene, |marks| marks.unavailable = true);
            return;
        };
        runtime.update(
            scene,
            terrain,
            resources.time().delta_time(),
            fade_time,
            strength,
        );
        let _ = this.write(scene, |marks| marks.runtime = Some(runtime));
    }
}

/// Gives the scene's first enabled terrain a [`ComponentTrackMarks`], unless
/// an object already has one.
pub(crate) fn attach(scene: &mut Scene) {
    if scene.objects_with::<ComponentTrackMarks>().next().is_some() {
        return;
    }
    let terrain = scene
        .objects_with::<ComponentTerrain>()
        .find(|&terrain| scene.is_component_enabled::<ComponentTerrain>(terrain));
    if let Some(terrain) = terrain {
        scene.add_component(terrain, ComponentTrackMarks::default());
    }
}

/// How dark a full-strength print makes each channel of the ground, as the
/// share of it taken off: a little more green than red, so grass reads
/// trodden into the soil, and dry ground reads pressed and shaded.
fn track_tint(palette: CombatPalette) -> [f32; 3] {
    match palette {
        CombatPalette::Sunny => [0.28, 0.36, 0.34],
        CombatPalette::Sunbaked => [0.26, 0.31, 0.34],
        CombatPalette::Moonlit => [0.36, 0.36, 0.34],
    }
}

/// The marks of one terrain while the scene plays: the field, the texture
/// its tiles are written to, and where each tank's treads last were.
struct MarksRuntime {
    field: MarksField,
    texture: Ref<Texture>,
    tint: [f32; 3],
    treads: HashMap<Uuid, TreadState>,
    /// The texel bytes of one tile, reused for every upload.
    texels: Vec<u8>,
    intensities: Vec<u8>,
}

impl MarksRuntime {
    /// Sets up marks over `terrain`: a field over its rectangle, a white
    /// texture for it, and a copy of its material sampling that texture,
    /// which the terrain draws from now on. `None`, logged, when the terrain
    /// has no material with the marks hook.
    fn create(
        scene: &mut Scene,
        terrain: GameObject,
        registries: &ReadOnlyRegistryContext,
        texels_per_metre: f32,
    ) -> Option<Self> {
        let (material, size) = scene
            .read_component::<ComponentTerrain, _, _>(terrain, |terrain| {
                (terrain.material.clone(), terrain.size)
            })?;
        let transform = scene.world_transform(terrain);
        let world_size = vec2(
            size.x * transform.scale.x.abs(),
            size.y * transform.scale.z.abs(),
        );
        if world_size.x <= 0.0 || world_size.y <= 0.0 {
            return None;
        }
        let rect = [
            transform.position.x - world_size.x * 0.5,
            transform.position.z - world_size.y * 0.5,
            world_size.x,
            world_size.y,
        ];
        let texels = |metres: f32| {
            ((metres * texels_per_metre.max(1.0)).ceil() as usize).clamp(1, MAX_TEXELS)
        };
        let field = MarksField::new(rect, texels(world_size.x), texels(world_size.y));
        let Some(material) = material.get_ref(registries) else {
            log::warn!("Track marks: the terrain has no material");
            return None;
        };

        let registry = registries.assets.read();
        let assets = registry.asset_context();
        let id = scene.uuid(terrain);
        let (width, height) = field.size();
        let texture_name = format!("runtime/track_marks/{id}/{width}x{height}");
        // One texture per terrain and size, cleared for each play: a texture
        // made anew under the same name would leave the renderer binding the
        // old one.
        let texture = match registry.load::<Texture>(&texture_name) {
            Ok(texture) => {
                let white = [255; 4].repeat(width * height);
                texture
                    .read()
                    .write_rgba8(0, 0, width as u32, height as u32, &white);
                texture
            }
            Err(_) => registry
                .create_or_update(
                    texture_name.clone(),
                    Texture::writable_rgba8(
                        assets.render_context.clone(),
                        &texture_name,
                        width as u32,
                        height as u32,
                        [255; 4],
                    ),
                )
                .ok()?,
        };
        let mut copy: Material = material.read().duplicate(&assets);
        let hooked = copy.set_variable(
            "marks_texture",
            ShaderVariableValue::Texture2D(MaterialTexture::Asset(AssetRef::from(Some(
                texture.clone(),
            )))),
        ) && copy.set_variable("marks_rect", ShaderVariableValue::Vec4(rect));
        if !hooked {
            log::warn!("Track marks: the terrain's material has no marks hook");
            return None;
        }
        let copy = registry
            .create_or_update(format!("runtime/track_marks/{id}/material"), copy)
            .ok()?;
        drop(registry);
        let _ = scene.write_component::<ComponentTerrain, _>(terrain, |terrain| {
            terrain.material = Some(copy).into();
        });
        Some(Self {
            field,
            texture,
            tint: track_tint(ComponentCombatPalette::in_scene(scene).palette),
            treads: HashMap::new(),
            texels: Vec::new(),
            intensities: Vec::new(),
        })
    }

    fn update(
        &mut self,
        scene: &Scene,
        terrain: GameObject,
        dt: f32,
        fade_time: f32,
        strength: f32,
    ) {
        let mut seen = HashSet::new();
        for tank in scene.objects_with::<ComponentTankController>() {
            let id = scene.uuid(tank);
            seen.insert(id);
            let transform = scene.world_transform(tank);
            let forward = transform.rotation * Vec3::z();
            let Some(pose) = TreadPose::new(
                vec2(transform.position.x, transform.position.z),
                vec2(forward.x, forward.z),
            ) else {
                continue;
            };
            let on_terrain = ComponentTerrain::sample(scene, terrain, pose.position)
                .is_some_and(|ground| (transform.position.y - ground.height) < GROUND_TOLERANCE);
            // Fog hides an unseen enemy; its prints would give it away.
            let prints = on_terrain && scene.is_visible_in_hierarchy(tank);
            let state = self
                .treads
                .entry(id)
                .or_insert_with(|| TreadState::new(pose));
            for print in state.advance(pose, strength) {
                if prints {
                    self.field.stamp(&print);
                }
            }
        }
        self.treads.retain(|id, _| seen.contains(id));
        self.field.advance(dt, fade_time);
        self.upload();
    }

    /// Writes every tile stamped or faded since the last upload to the
    /// texture.
    fn upload(&mut self) {
        let lut = encode_table(self.tint);
        let texture = self.texture.read();
        for tile in self.field.take_dirty() {
            self.field.read_tile(tile, &mut self.intensities);
            self.texels.clear();
            self.texels.extend(
                self.intensities
                    .iter()
                    .flat_map(|&intensity| lut[intensity as usize]),
            );
            texture.write_rgba8(
                tile.x as u32,
                tile.y as u32,
                tile.width as u32,
                tile.height as u32,
                &self.texels,
            );
        }
    }
}

/// The texel each mark strength encodes to: the ground's colour multiplier,
/// white for none, `tint` taken off for a full-strength mark.
fn encode_table(tint: [f32; 3]) -> Vec<[u8; 4]> {
    (0..=255u32)
        .map(|intensity| {
            let share = intensity as f32 / 255.0;
            let channel = |tint: f32| ((1.0 - tint * share) * 255.0).round() as u8;
            [channel(tint[0]), channel(tint[1]), channel(tint[2]), 255]
        })
        .collect()
}

/// Where a hull stands on the ground, seen from above.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TreadPose {
    /// World X and Z of the hull's centre.
    pub position: Vec2,
    /// Unit heading over the ground, in X and Z.
    pub forward: Vec2,
}

impl TreadPose {
    /// The pose at `position` facing `forward`, or `None` when the hull faces
    /// straight up or down.
    pub fn new(position: Vec2, forward: Vec2) -> Option<Self> {
        let length = forward.norm();
        (length > 1e-4).then(|| Self {
            position,
            forward: forward / length,
        })
    }

    /// The hull's right, over the ground.
    fn right(&self) -> Vec2 {
        vec2(self.forward.y, -self.forward.x)
    }

    /// The centre of the tread on `side`: -1 left, 1 right.
    fn tread(&self, side: f32) -> Vec2 {
        self.position + self.right() * (side * TREAD_OFFSET)
    }
}

/// Where one tank's treads last printed, and how far each has rolled.
pub(crate) struct TreadState {
    pose: TreadPose,
    /// Signed distance each tread, left then right, has rolled along its
    /// heading: it keeps the grousers of a print still on the ground while
    /// the tread rolls over them.
    rolled: [f32; 2],
}

impl TreadState {
    pub fn new(pose: TreadPose) -> Self {
        Self {
            pose,
            rolled: [0.0; 2],
        }
    }

    /// Moves the treads to `pose` and returns the two prints they make
    /// there at `strength`: none when the hull barely moved or jumped away,
    /// as on a respawn.
    pub fn advance(&mut self, pose: TreadPose, strength: f32) -> Vec<TreadPrint> {
        let step = (pose.position - self.pose.position).norm();
        if step > MAX_STEP {
            *self = Self::new(pose);
            return Vec::new();
        }
        let turn = self.pose.forward.dot(&pose.forward).clamp(-1.0, 1.0).acos();
        if step < MIN_MOVE && turn < MIN_TURN {
            return Vec::new();
        }
        let prints = [-1.0f32, 1.0]
            .into_iter()
            .zip(&mut self.rolled)
            .map(|(side, rolled)| {
                let from = self.pose.tread(side);
                let to = pose.tread(side);
                *rolled += (to - from).dot(&pose.forward);
                TreadPrint {
                    center: to,
                    forward: pose.forward,
                    half_length: TREAD_LENGTH * 0.5,
                    half_width: TREAD_WIDTH * 0.5,
                    rolled: *rolled,
                    strength,
                }
            })
            .collect();
        self.pose = pose;
        prints
    }
}

/// One tread's contact patch on the ground: an oriented rectangle.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TreadPrint {
    pub center: Vec2,
    /// Unit direction the tread runs along.
    pub forward: Vec2,
    pub half_length: f32,
    pub half_width: f32,
    /// How far the tread has rolled; see [`TreadState`].
    pub rolled: f32,
    /// How dark the print is, from 0 to 1.
    pub strength: f32,
}

/// The texels of one tile: its corner and size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TileRect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

#[derive(Clone, Copy, Default)]
struct TileState {
    /// Whether any texel of the tile holds a mark.
    active: bool,
    /// Whether the tile changed since the last upload.
    dirty: bool,
    /// Seconds since the tile last faded.
    clock: f32,
    /// The share of a strength level the tile still owes its fade.
    residue: f32,
}

/// Mark strength over a world rectangle of the ground, one byte per texel,
/// faded and uploaded by [`TILE`]-texel tiles. Texel (0, 0) covers the
/// rectangle's corner at its lowest X and Z; columns run along +X and rows
/// along +Z, as the ground shader's `marks_rect` maps them.
pub(crate) struct MarksField {
    origin: Vec2,
    /// Texels per metre along X and Z.
    scale: Vec2,
    width: usize,
    height: usize,
    intensity: Vec<u8>,
    tiles_x: usize,
    tiles: Vec<TileState>,
    /// The active tiles' indices.
    active: Vec<usize>,
}

impl MarksField {
    /// An empty field of `width` by `height` texels over `rect`: X and Z of
    /// its lowest corner, then its width and depth, in metres.
    pub fn new(rect: [f32; 4], width: usize, height: usize) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        let tiles_x = width.div_ceil(TILE);
        let tiles_y = height.div_ceil(TILE);
        Self {
            origin: vec2(rect[0], rect[1]),
            scale: vec2(width as f32 / rect[2], height as f32 / rect[3]),
            width,
            height,
            intensity: vec![0; width * height],
            tiles_x,
            tiles: vec![TileState::default(); tiles_x * tiles_y],
            active: Vec::new(),
        }
    }

    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// How many tiles hold marks.
    #[cfg(test)]
    pub fn active_tiles(&self) -> usize {
        self.active.len()
    }

    /// The mark strength at world X, Z `point`, 0 outside the field.
    #[cfg(test)]
    pub fn intensity_at(&self, point: Vec2) -> u8 {
        let texel = (point - self.origin).component_mul(&self.scale);
        if texel.x < 0.0 || texel.y < 0.0 {
            return 0;
        }
        let (x, y) = (texel.x as usize, texel.y as usize);
        if x >= self.width || y >= self.height {
            return 0;
        }
        self.intensity[y * self.width + x]
    }

    /// Prints `print` into the field: each texel keeps the stronger of its
    /// mark and the print's.
    pub fn stamp(&mut self, print: &TreadPrint) {
        let side = vec2(print.forward.y, -print.forward.x);
        let reach = vec2(
            print.forward.x.abs() * print.half_length + side.x.abs() * print.half_width,
            print.forward.y.abs() * print.half_length + side.y.abs() * print.half_width,
        );
        let low = (print.center - reach - self.origin).component_mul(&self.scale);
        let high = (print.center + reach - self.origin).component_mul(&self.scale);
        let x0 = (low.x.floor() - 1.0).max(0.0) as usize;
        let y0 = (low.y.floor() - 1.0).max(0.0) as usize;
        let x1 = ((high.x.ceil() + 1.0).max(0.0) as usize).min(self.width);
        let y1 = ((high.y.ceil() + 1.0).max(0.0) as usize).min(self.height);
        // Coverage falls off over one texel at the print's edges.
        let texel = 1.0 / self.scale.x.min(self.scale.y);
        let strength = print.strength.clamp(0.0, 1.0) * 255.0;
        for y in y0..y1 {
            for x in x0..x1 {
                let point = self.origin
                    + vec2(
                        (x as f32 + 0.5) / self.scale.x,
                        (y as f32 + 0.5) / self.scale.y,
                    );
                let offset = point - print.center;
                let along = offset.dot(&print.forward);
                let across = offset.dot(&side);
                let cover = |half: f32, distance: f32| {
                    ((half - distance.abs()) / texel + 0.5).clamp(0.0, 1.0)
                };
                let coverage = cover(print.half_length, along) * cover(print.half_width, across);
                if coverage <= 0.0 {
                    continue;
                }
                let phase = ((along + print.rolled) / GROUSER_PITCH).rem_euclid(1.0);
                let grouser = 1.0 - GROUSER_DEPTH * (1.0 - 2.0 * (phase - 0.5).abs());
                let value = (strength * coverage * grouser).round() as u8;
                let index = y * self.width + x;
                if value > self.intensity[index] {
                    self.intensity[index] = value;
                    self.touch(x / TILE + (y / TILE) * self.tiles_x);
                }
            }
        }
    }

    /// Marks tile `index` changed, and active if it was not.
    fn touch(&mut self, index: usize) {
        let tile = &mut self.tiles[index];
        tile.dirty = true;
        if !tile.active {
            tile.active = true;
            // Spread the tiles' fade steps over the step, so they do not all
            // upload on one frame.
            tile.clock = (index as f32 * 0.618_034).fract() * FADE_STEP;
            tile.residue = 0.0;
            self.active.push(index);
        }
    }

    /// Lets `dt` seconds pass: each active tile due a fade step loses the
    /// strength the time since its last step is worth, a full-strength mark
    /// fading away in `fade_time` seconds. A tile left without marks drops
    /// out of the active tiles.
    pub fn advance(&mut self, dt: f32, fade_time: f32) {
        let mut index = 0;
        while index < self.active.len() {
            let tile_index = self.active[index];
            let rect = self.tile_rect(tile_index);
            let tile = &mut self.tiles[tile_index];
            tile.clock += dt.max(0.0);
            if tile.clock < FADE_STEP {
                index += 1;
                continue;
            }
            let levels = if fade_time > 0.0 {
                tile.clock * 255.0 / fade_time + tile.residue
            } else {
                255.0
            };
            tile.clock = 0.0;
            let fade = levels.floor().min(255.0);
            tile.residue = levels - fade;
            let fade = fade as u8;
            let mut any = false;
            if fade > 0 {
                tile.dirty = true;
                for y in rect.y..rect.y + rect.height {
                    let row = &mut self.intensity[y * self.width + rect.x..][..rect.width];
                    for value in row {
                        *value = value.saturating_sub(fade);
                        any |= *value > 0;
                    }
                }
            } else {
                any = true;
            }
            if any {
                index += 1;
            } else {
                tile.active = false;
                self.active.swap_remove(index);
            }
        }
    }

    /// The tiles changed since the last call, each once.
    pub fn take_dirty(&mut self) -> Vec<TileRect> {
        let dirty = self
            .tiles
            .iter_mut()
            .enumerate()
            .filter(|(_, tile)| tile.dirty)
            .map(|(index, tile)| {
                tile.dirty = false;
                index
            })
            .collect::<Vec<_>>();
        dirty
            .into_iter()
            .map(|index| self.tile_rect(index))
            .collect()
    }

    /// Copies `tile`'s strengths, row by row, into `out`.
    pub fn read_tile(&self, tile: TileRect, out: &mut Vec<u8>) {
        out.clear();
        for y in tile.y..tile.y + tile.height {
            out.extend_from_slice(&self.intensity[y * self.width + tile.x..][..tile.width]);
        }
    }

    fn tile_rect(&self, index: usize) -> TileRect {
        let x = (index % self.tiles_x) * TILE;
        let y = (index / self.tiles_x) * TILE;
        TileRect {
            x,
            y,
            width: TILE.min(self.width - x),
            height: TILE.min(self.height - y),
        }
    }
}

#[cfg(test)]
mod tests;
