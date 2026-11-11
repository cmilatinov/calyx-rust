use crate::assets::material::{Material, ShaderVariableValue};
use crate::assets::mesh::Mesh;
use crate::assets::AssetId;
use crate::component::{ComponentDirectionalLight, ComponentMesh, ComponentWater};
use crate::context::{ReadOnlyAssetContext, ReadOnlyRegistryContext};
use crate::core::Ref;
use crate::math::{water_region, waterline, BankProfile, WaterPath, WaterPathPoint};
use crate::render::static_batch::touched_mesh_objects;
use crate::render::{SceneRendererError, Shader};
use crate::scene::{GameObject, Scene, SceneId};
use egui::{Color32, Rgba};
use legion::{Entity, IntoQuery};
use nalgebra_glm::{Mat4, Vec2, Vec3};
use std::collections::HashMap;
use std::time::Instant;
use uuid::Uuid;

/// The water material's own shader, whose group 3 the water's geometry pass
/// binds.
pub(crate) const WATER_SHADER: &str = "shaders/mesh/water";

/// Longest step the water's clock takes in one frame, in seconds, so a
/// stall does not jump the waves forward.
const MAX_TIME_STEP: f64 = 0.25;
/// The water's clock wraps after this many seconds, so it keeps its
/// precision as an `f32` in the shader; the waves jump once each wrap.
const TIME_WRAP: f64 = 3600.0;

/// The light the water glints with: the scene's brightest enabled
/// directional light.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WaterLight {
    /// World direction the light travels along.
    pub direction: Vec3,
    /// Its colour, as the light manager hands it to the lighting.
    pub color: [f32; 3],
}

impl WaterLight {
    /// The brightest enabled directional light of `scene`, or `None` when it
    /// has none.
    pub fn of_scene(scene: &Scene) -> Option<Self> {
        let mut query = <(Entity, &ComponentDirectionalLight)>::query();
        query
            .iter(scene.world())
            .filter_map(|(entity, light)| Some((scene.game_object_from_entity(*entity)?, light)))
            .filter(|(object, _)| scene.is_component_enabled::<ComponentDirectionalLight>(*object))
            .max_by(|(_, a), (_, b)| a.intensity.total_cmp(&b.intensity))
            .map(|(object, light)| {
                let color = light.color.to_normalized_gamma_f32();
                Self {
                    direction: scene
                        .world_transform(object)
                        .transform_direction(&Vec3::z_axis()),
                    color: [color[0], color[1], color[2]],
                }
            })
    }
}

/// The water's clock after `seconds` more from `time`, wrapped to
/// `[0, TIME_WRAP)`.
pub(crate) fn advance_water_time(time: f64, seconds: f64) -> f64 {
    (time + seconds.max(0.0)).rem_euclid(TIME_WRAP)
}

/// A linear RGBA colour for a material, from an sRGB colour.
fn linear(color: Color32) -> ShaderVariableValue {
    ShaderVariableValue::Color(Rgba::from(color).to_array())
}

/// How far past a water's shores, in local units, a mesh is still cut for a
/// waterline.
const REGION_MARGIN: f32 = 1.0;

/// What the meshes are built from, bit for bit: the path's points, cell
/// size, obstacles, waterlines and openings, and the bank profile.
fn mesh_key(path: &WaterPath, banks: &BankProfile) -> Vec<u32> {
    let mut key = vec![path.cell_size.to_bits(), path.points.len() as u32];
    for point in &path.points {
        key.extend([point.position.x, point.position.y, point.width].map(f32::to_bits));
    }
    for circle in &path.obstacles {
        key.extend([circle.center.x, circle.center.y, circle.radius].map(f32::to_bits));
    }
    key.push(path.waterlines.len() as u32);
    for outline in &path.waterlines {
        key.push(outline.len() as u32);
        key.extend(outline.iter().flat_map(|p| [p.x.to_bits(), p.y.to_bits()]));
    }
    key.push(path.openings.len() as u32);
    for circle in &path.openings {
        key.extend([circle.center.x, circle.center.y, circle.radius].map(f32::to_bits));
    }
    key.extend([banks.height, banks.depth, banks.run, banks.lip].map(f32::to_bits));
    key
}

struct CachedWater {
    key: Vec<u32>,
    mesh: Option<Ref<Mesh>>,
    banks: Option<Ref<Mesh>>,
    material: Ref<Material>,
    /// This water's clock, in seconds.
    time: f64,
    /// Where the scene's meshes cut this water's surface.
    waterlines: WaterWaterlines,
    seen: bool,
}

/// Where the scene's mesh objects cut one water's rest surface, kept from
/// frame to frame and updated from the scene's changes.
#[derive(Default)]
struct WaterWaterlines {
    /// The water's world-to-local transform and points they were cut for.
    from_world: Option<Mat4>,
    points: Vec<WaterPathPoint>,
    /// The water's local X and Z box, widened by [`REGION_MARGIN`]: a mesh
    /// clear of it cuts nothing.
    region: (Vec2, Vec2),
    /// Each object's pieces, by entity, with its id, which orders them.
    pieces: HashMap<Entity, (Uuid, Vec<Vec<Vec2>>)>,
    /// Every object's pieces, in order of their ids, as the path takes them.
    outlines: Vec<Vec<Vec2>>,
}

/// The scene's changes the waterlines follow from one frame to the next.
#[derive(Default)]
struct WaterlineChanges {
    scene: Option<SceneId>,
    cursor: u64,
    epoch: Option<u64>,
    /// Whether every water cuts the whole scene again this frame: for
    /// another scene, a journal the cursor fell behind, or a registry change.
    rescan: bool,
    /// The objects whose meshes may cut the water differently since the
    /// last frame.
    touched: Vec<Entity>,
    /// Each mesh's local box, by mesh, `None` for one without a finite
    /// vertex.
    bounds: HashMap<AssetId, Option<(Vec3, Vec3)>>,
}

impl WaterlineChanges {
    /// Reads the scene's changes since the last frame. `following` is
    /// whether any water draws, without which nothing is read.
    fn begin_frame(
        &mut self,
        registries: &ReadOnlyRegistryContext,
        scene: &Scene,
        following: bool,
    ) {
        let epoch = registries.assets.read().change_epoch();
        let journal = scene.change_journal();
        self.touched.clear();
        let changes = (following && self.scene == Some(scene.id()))
            .then(|| journal.since(self.cursor))
            .flatten();
        self.rescan = match changes {
            Some(changes) => {
                self.touched = touched_mesh_objects(scene, changes);
                false
            }
            None => true,
        };
        if self.epoch != Some(epoch) {
            self.rescan = true;
            self.bounds.clear();
        }
        self.scene = Some(scene.id());
        self.cursor = journal.end();
        self.epoch = Some(epoch);
    }

    /// The pieces `entity`'s mesh cuts through the water whose
    /// world-to-local transform is `from_world` inside `region`, with the
    /// object's id, or `None` when it cuts none: it is hidden, its mesh is
    /// disabled or missing, or it stands clear of the surface.
    fn cut(
        &mut self,
        registries: &ReadOnlyRegistryContext,
        scene: &Scene,
        entity: Entity,
        from_world: &Mat4,
        (least, greatest): (Vec2, Vec2),
    ) -> Option<(Uuid, Vec<Vec<Vec2>>)> {
        let object = scene.game_object_from_entity(entity)?;
        let source = scene.read_component::<ComponentMesh, _, _>(object, |c| c.mesh.clone())?;
        if !scene.is_component_enabled::<ComponentMesh>(object)
            || !scene.is_visible_in_hierarchy(object)
        {
            return None;
        }
        let mesh = source.get_ref(registries)?;
        let (low, high) = (*self.bounds.entry(mesh.id()).or_insert_with(|| {
            let mesh = mesh.read();
            let mut vertices = mesh
                .vertices
                .iter()
                .copied()
                .filter(|p| p.iter().all(|c| c.is_finite()));
            let first = vertices.next()?;
            Some(vertices.fold((first, first), |(a, b), p| (a.inf(&p), b.sup(&p))))
        }))?;
        let to_water = from_world * scene.world_transform(object).matrix();
        let mut box_least = Vec3::repeat(f32::MAX);
        let mut box_greatest = Vec3::repeat(f32::MIN);
        for i in 0..8 {
            let corner = Vec3::new(
                if i & 1 == 0 { low.x } else { high.x },
                if i & 2 == 0 { low.y } else { high.y },
                if i & 4 == 0 { low.z } else { high.z },
            );
            let at = (to_water * corner.push(1.0)).xyz();
            box_least = box_least.inf(&at);
            box_greatest = box_greatest.sup(&at);
        }
        let clear = box_least.y >= 0.0
            || box_greatest.y < 0.0
            || box_greatest.x < least.x
            || box_least.x > greatest.x
            || box_greatest.z < least.y
            || box_least.z > greatest.y;
        if clear {
            return None;
        }
        let mesh = mesh.read();
        let pieces = waterline(&mesh.vertices, &mesh.indices, &to_water);
        (!pieces.is_empty()).then(|| (scene.uuid(object), pieces))
    }
}

impl WaterWaterlines {
    /// Brings the pieces up to date for the water `path` of `object`: cuts
    /// the whole scene again when the water moved, changed shape or
    /// `changes` asks it to, else only the objects that changed.
    fn update(
        &mut self,
        registries: &ReadOnlyRegistryContext,
        scene: &Scene,
        object: GameObject,
        path: &WaterPath,
        changes: &mut WaterlineChanges,
    ) {
        let Some(from_world) = scene.world_transform(object).matrix().try_inverse() else {
            self.clear();
            return;
        };
        let moved = self.from_world != Some(from_world) || self.points != path.points;
        let mut changed = false;
        if moved || changes.rescan {
            let Some(sections) = path.sections() else {
                self.clear();
                return;
            };
            let (least, greatest) = water_region(&sections);
            self.region = (
                least - Vec2::repeat(REGION_MARGIN),
                greatest + Vec2::repeat(REGION_MARGIN),
            );
            self.from_world = Some(from_world);
            self.points = path.points.clone();
            self.pieces.clear();
            let mut query = <(Entity, &ComponentMesh)>::query();
            let entities: Vec<Entity> = query.iter(scene.world()).map(|(e, _)| *e).collect();
            for entity in entities {
                if let Some(pieces) =
                    changes.cut(registries, scene, entity, &from_world, self.region)
                {
                    self.pieces.insert(entity, pieces);
                }
            }
            changed = true;
        } else {
            for i in 0..changes.touched.len() {
                let entity = changes.touched[i];
                let pieces = changes.cut(registries, scene, entity, &from_world, self.region);
                let had = self.pieces.contains_key(&entity);
                match pieces {
                    Some(pieces) => {
                        if self.pieces.get(&entity) != Some(&pieces) {
                            self.pieces.insert(entity, pieces);
                            changed = true;
                        }
                    }
                    None if had => {
                        self.pieces.remove(&entity);
                        changed = true;
                    }
                    None => {}
                }
            }
        }
        if changed {
            let mut pieces: Vec<&(Uuid, Vec<Vec<Vec2>>)> = self.pieces.values().collect();
            pieces.sort_by_key(|(id, _)| *id);
            self.outlines = pieces
                .into_iter()
                .flat_map(|(_, pieces)| pieces.iter().cloned())
                .collect();
        }
    }

    fn clear(&mut self) {
        *self = Self::default();
    }
}

/// What a water draws this frame.
pub(crate) struct WaterDraw {
    /// The surface, with the water material.
    pub mesh: Ref<Mesh>,
    pub material: Ref<Material>,
    /// The banks, when the profile has walls; drawn with the component's
    /// bank material.
    pub banks: Option<Ref<Mesh>>,
}

/// The meshes and material of each water a scene renderer draws, one per
/// water owner. The meshes are built on the CPU once and rebuilt only when
/// the owner's path, obstacles or bank profile change; every displacement
/// happens in the water shader. The material, the renderer's own on the
/// water shader, takes the component's colours, waves and the water's clock
/// every frame, so inspector edits show at once.
#[derive(Default)]
pub(super) struct WaterCache {
    scene: Option<SceneId>,
    shader: Option<Ref<Shader>>,
    /// When the last frame was collected, which times the water.
    last_frame: Option<Instant>,
    /// Seconds since the last frame, at most [`MAX_TIME_STEP`].
    step: f64,
    waters: HashMap<Uuid, CachedWater>,
    /// The scene's changes the waters' waterlines follow.
    changes: WaterlineChanges,
}

impl WaterCache {
    pub fn begin_frame(&mut self, context: &ReadOnlyAssetContext, scene: &Scene) {
        if self.scene != Some(scene.id()) {
            self.waters.clear();
            self.scene = Some(scene.id());
        }
        self.changes
            .begin_frame(&context.registries, scene, !self.waters.is_empty());
        let now = Instant::now();
        self.step = self
            .last_frame
            .map_or(0.0, |last| (now - last).as_secs_f64().min(MAX_TIME_STEP));
        self.last_frame = Some(now);
        for water in self.waters.values_mut() {
            water.seen = false;
        }
    }

    /// What `water`, on `object` of `scene` with the id `owner`, lit by
    /// `light`, draws, or `None` when its path is not valid or the water
    /// shader did not load. Its foam rings what the scene's meshes cut of
    /// its surface.
    pub fn draw(
        &mut self,
        context: &ReadOnlyAssetContext,
        scene: &Scene,
        object: GameObject,
        owner: Uuid,
        water: &ComponentWater,
        light: Option<&WaterLight>,
    ) -> Option<WaterDraw> {
        let mut path = water.path();
        let profile = water.bank_profile();
        if !self.waters.contains_key(&owner) {
            let shader = self.shader(context)?;
            let material = Material::from_shader(context, shader);
            // The draw list batches and looks materials up by id, so each
            // water's material gets its own.
            self.waters.insert(
                owner,
                CachedWater {
                    key: Vec::new(),
                    mesh: None,
                    banks: None,
                    material: Ref::from_id_value(Uuid::new_v4(), material),
                    time: 0.0,
                    waterlines: Default::default(),
                    seen: true,
                },
            );
        }
        let step = self.step;
        let cached = self.waters.get_mut(&owner)?;
        cached
            .waterlines
            .update(&context.registries, scene, object, &path, &mut self.changes);
        path.waterlines = cached.waterlines.outlines.clone();
        let key = mesh_key(&path, &profile);
        if cached.mesh.is_none() || cached.key != key {
            cached.mesh = build_water_mesh(context, &path);
            cached.banks = build_bank_mesh(context, &path, &profile);
            cached.key = key;
        }
        cached.seen = true;
        cached.time = advance_water_time(cached.time, step);
        write_material(&mut cached.material.write(), water, cached.time, light);
        Some(WaterDraw {
            mesh: cached.mesh.clone()?,
            material: cached.material.clone(),
            banks: cached.banks.clone(),
        })
    }

    pub fn end_frame(&mut self) {
        self.waters.retain(|_, water| water.seen);
    }

    fn shader(&mut self, context: &ReadOnlyAssetContext) -> Option<Ref<Shader>> {
        if self.shader.is_none() {
            match SceneRendererError::load::<Shader>(context, WATER_SHADER) {
                Ok(shader) => self.shader = Some(shader),
                Err(error) => {
                    crate::logging::warn_once("water-shader".to_string(), || {
                        format!("Water draws nothing: {error}")
                    });
                    return None;
                }
            }
        }
        self.shader.clone()
    }
}

/// Fills the water material from `water`, at `time` on its clock.
fn write_material(
    material: &mut Material,
    water: &ComponentWater,
    time: f64,
    light: Option<&WaterLight>,
) {
    let (direction, glint) = match light {
        Some(light) => (
            light.direction,
            light
                .color
                .map(|channel| channel * water.glint_strength.max(0.0)),
        ),
        None => (-Vec3::y(), [0.0; 3]),
    };
    let float = ShaderVariableValue::Float;
    for (name, value) in [
        ("deep_color", linear(water.deep_color)),
        ("shallow_color", linear(water.shallow_color)),
        ("foam_color", linear(water.foam_color)),
        (
            "glint_color",
            ShaderVariableValue::Color([glint[0], glint[1], glint[2], 1.0]),
        ),
        (
            "light_direction",
            ShaderVariableValue::Vec4([direction.x, direction.y, direction.z, 0.0]),
        ),
        ("wave_amplitude", float(water.wave_amplitude)),
        ("wave_length", float(water.wave_length)),
        ("flow_speed", float(water.flow_speed)),
        ("time", float(time as f32)),
        ("shore_distance", float(water.shore_distance)),
        ("foam_width", float(water.foam_width)),
        ("foam_patches", float(water.foam_patches)),
        ("obstacle_foam", float(water.obstacle_foam)),
        ("obstacle_shallows", float(water.obstacle_shallows)),
        ("facet_variation", float(water.facet_variation)),
        ("roughness", float(water.roughness)),
        (
            "outline_style",
            ShaderVariableValue::Uint(water.outline_style),
        ),
        (
            "shading",
            ShaderVariableValue::Uint(water.shading.shader_value()),
        ),
    ] {
        material.set_variable(name, value);
    }
}

fn build_water_mesh(context: &ReadOnlyAssetContext, path: &WaterPath) -> Option<Ref<Mesh>> {
    let geometry = path.geometry()?;
    let mut mesh = Mesh::new(&context.render_context);
    // The water shader reads its own attributes from the mesh channels; see
    // `WaterGeometry`.
    mesh.vertices = geometry.positions;
    mesh.normals = geometry.facets;
    mesh.tangents = geometry.extra;
    mesh.bone_weights = geometry.footprint;
    mesh.uvs[0] = geometry.river;
    mesh.uvs[1] = geometry.next;
    mesh.uvs[2] = geometry.previous;
    mesh.uvs[3] = geometry.next_river;
    mesh.indices = geometry.indices;
    mesh.mark_dirty();
    Some(Ref::from_id_value(Uuid::new_v4(), mesh))
}

fn build_bank_mesh(
    context: &ReadOnlyAssetContext,
    path: &WaterPath,
    profile: &BankProfile,
) -> Option<Ref<Mesh>> {
    let geometry = path.banks(profile)?;
    let mut mesh = Mesh::new(&context.render_context);
    mesh.tangents = vec![Mesh::DEFAULT_TANGENT; geometry.positions.len()];
    mesh.vertices = geometry.positions;
    mesh.normals = geometry.normals;
    mesh.indices = geometry.indices;
    mesh.mark_dirty();
    Some(Ref::from_id_value(Uuid::new_v4(), mesh))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::AssetAccess;
    use crate::component::WaterObstacle;
    use crate::test_support::{SceneBuilder, TestAssets};
    use approx::assert_abs_diff_eq;
    use nalgebra_glm::vec2;

    #[test]
    fn the_waters_clock_runs_on_and_wraps() {
        assert_abs_diff_eq!(advance_water_time(1.0, 0.5), 1.5);
        assert_abs_diff_eq!(
            advance_water_time(TIME_WRAP - 0.25, 0.5),
            0.25,
            epsilon = 1e-9
        );
        assert_eq!(advance_water_time(2.0, -1.0), 2.0);
    }

    #[test]
    fn cache_builds_the_meshes_once_and_fills_the_material_each_frame() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let mut water = ComponentWater::default();
        let owner = Uuid::new_v4();
        let mut scene = context.scene();
        let object = scene.create(None, None);
        let light = WaterLight {
            direction: -Vec3::y(),
            color: [1.0, 0.5, 0.25],
        };
        let mut cache = WaterCache::default();
        cache.begin_frame(&context, &scene);
        let first = cache
            .draw(&context, &scene, object, owner, &water, Some(&light))
            .unwrap();
        let vertices = first.mesh.read().vertices.len();
        assert_eq!(vertices, water.path().geometry().unwrap().positions.len());
        assert_eq!(
            first.banks.as_ref().unwrap().read().vertices.len(),
            water
                .path()
                .banks(&water.bank_profile())
                .unwrap()
                .positions
                .len()
        );
        let shader: Ref<Shader> = SceneRendererError::load(&context, WATER_SHADER).unwrap();
        assert_eq!(first.material.read().shader.id(), shader.id());
        cache.end_frame();

        // Colours and waves reach the same material without a rebuild.
        cache.begin_frame(&context, &scene);
        water.wave_amplitude = 0.3;
        water.deep_color = Color32::from_rgb(0x20, 0x40, 0x60);
        water.glint_strength = 2.0;
        let same = cache
            .draw(&context, &scene, object, owner, &water, Some(&light))
            .unwrap();
        assert_eq!(same.mesh.id(), first.mesh.id());
        assert_eq!(same.material.id(), first.material.id());
        let material = first.material.clone();
        let variable = |name: &str| {
            material
                .read()
                .variables
                .iter()
                .find(|variable| variable.name == name)
                .map(|variable| variable.value.as_slice().to_vec())
                .unwrap()
        };
        let bytes = |value: ShaderVariableValue| value.as_slice().to_vec();
        assert_eq!(
            variable("wave_amplitude"),
            bytes(ShaderVariableValue::Float(0.3))
        );
        assert_eq!(
            variable("deep_color"),
            bytes(ShaderVariableValue::Color(
                Rgba::from(water.deep_color).to_array()
            ))
        );
        assert_eq!(
            variable("glint_color"),
            bytes(ShaderVariableValue::Color([2.0, 1.0, 0.5, 1.0]))
        );
        assert_eq!(material.read().outline_style(), 0);

        // A new point, obstacle or bank rebuilds the meshes.
        water.points[1].position.x += 1.0;
        let moved = cache
            .draw(&context, &scene, object, owner, &water, None)
            .unwrap();
        assert_ne!(moved.mesh.id(), first.mesh.id());
        water.obstacles.push(WaterObstacle {
            position: vec2(0.0, 0.0),
            radius: 1.0,
        });
        let ringed = cache
            .draw(&context, &scene, object, owner, &water, None)
            .unwrap();
        assert_ne!(ringed.mesh.id(), moved.mesh.id());
        assert_eq!(
            variable("glint_color"),
            bytes(ShaderVariableValue::Color([0.0, 0.0, 0.0, 1.0])),
            "no light, no glints"
        );
        water.bank_height = 0.0;
        water.bank_depth = 0.0;
        assert!(cache
            .draw(&context, &scene, object, owner, &water, None)
            .unwrap()
            .banks
            .is_none());
        water.points.truncate(1);
        assert!(cache
            .draw(&context, &scene, object, owner, &water, None)
            .is_none());
        cache.end_frame();

        cache.begin_frame(&context, &scene);
        cache.end_frame();
        assert!(cache.waters.is_empty());
    }

    /// Whether any facet of `mesh`, a water's, knows a footprint.
    fn rings_anything(mesh: &Ref<Mesh>) -> bool {
        mesh.read().tangents.iter().any(|extra| extra.w >= 0.0)
    }

    /// A mesh standing in the water is ringed, one clear of its surface or
    /// away from it is not, and moving it out of the water takes its ring
    /// away.
    #[test]
    fn the_water_rings_the_meshes_that_cross_its_surface() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let cube = context.registries.assets.read().cube().expect("cube");
        let material = context
            .registries
            .assets
            .read()
            .create(
                "test/water_rock".into(),
                Material::from_shader(
                    &context,
                    context
                        .registries
                        .assets
                        .read()
                        .load::<Shader>("shaders/mesh/pbr")
                        .unwrap(),
                ),
            )
            .unwrap();
        let mut builder = SceneBuilder::new(context.registries.clone());
        let river = builder.spawn("river");
        let water = ComponentWater::default();
        builder.add_component(river, water.clone());
        let mut spawn = |name: &str, position: Vec3| {
            let object = builder.spawn(name);
            builder.add_component(
                object,
                crate::component::ComponentMesh {
                    mesh: crate::assets::AssetRef::from_id(cube.id()),
                    material: crate::assets::AssetRef::from_id(material.id()),
                    ..Default::default()
                },
            );
            builder.set_transform(object, nalgebra_glm::translation(&position));
            object
        };
        // A rock in the middle of the river, a deck above it and a cube
        // crossing the plane far from the water.
        let rock = spawn("rock", Vec3::new(0.0, 0.0, 0.0));
        spawn("deck", Vec3::new(0.0, 2.0, 4.0));
        spawn("far", Vec3::new(40.0, 0.0, 0.0));
        let mut scene = builder.finish();
        let owner = scene.uuid(river);
        let mut cache = WaterCache::default();
        cache.begin_frame(&context, &scene);
        let ringed = cache
            .draw(&context, &scene, river, owner, &water, None)
            .unwrap();
        cache.end_frame();
        let outlines = &cache.waters[&owner].waterlines.outlines;
        assert_eq!(outlines.len(), 1, "only the rock: {outlines:?}");
        assert!(outlines[0].iter().all(|p| p.norm() < 1.5));
        assert!(rings_anything(&ringed.mesh));

        // Nothing changed: the same mesh.
        cache.begin_frame(&context, &scene);
        let same = cache
            .draw(&context, &scene, river, owner, &water, None)
            .unwrap();
        cache.end_frame();
        assert_eq!(same.mesh.id(), ringed.mesh.id());

        // Lifted clear of the water, the rock is ringed no more.
        scene.set_transform(rock, &nalgebra_glm::translation(&Vec3::new(0.0, 3.0, 0.0)));
        cache.begin_frame(&context, &scene);
        let clear = cache
            .draw(&context, &scene, river, owner, &water, None)
            .unwrap();
        cache.end_frame();
        assert_ne!(clear.mesh.id(), ringed.mesh.id());
        assert!(cache.waters[&owner].waterlines.outlines.is_empty());
        assert!(!rings_anything(&clear.mesh));
    }
}
