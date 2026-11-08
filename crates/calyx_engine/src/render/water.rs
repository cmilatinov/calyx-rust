use crate::assets::material::{Material, ShaderVariableValue};
use crate::assets::mesh::Mesh;
use crate::component::{ComponentDirectionalLight, ComponentWater};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::math::{BankProfile, WaterPath};
use crate::render::{SceneRendererError, Shader};
use crate::scene::{Scene, SceneId};
use egui::{Color32, Rgba};
use legion::{Entity, IntoQuery};
use nalgebra_glm::Vec3;
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

/// What the meshes are built from, bit for bit: the path's points, cell
/// size and obstacles, and the bank profile.
fn mesh_key(path: &WaterPath, banks: &BankProfile) -> Vec<u32> {
    let mut key = vec![path.cell_size.to_bits(), path.points.len() as u32];
    for point in &path.points {
        key.extend([point.position.x, point.position.y, point.width].map(f32::to_bits));
    }
    for circle in &path.obstacles {
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
    seen: bool,
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
}

impl WaterCache {
    pub fn begin_frame(&mut self, scene: SceneId) {
        if self.scene != Some(scene) {
            self.waters.clear();
            self.scene = Some(scene);
        }
        let now = Instant::now();
        self.step = self
            .last_frame
            .map_or(0.0, |last| (now - last).as_secs_f64().min(MAX_TIME_STEP));
        self.last_frame = Some(now);
        for water in self.waters.values_mut() {
            water.seen = false;
        }
    }

    /// What `owner`'s `water`, lit by `light`, draws, or `None` when its path
    /// is not valid or the water shader did not load.
    pub fn draw(
        &mut self,
        context: &ReadOnlyAssetContext,
        owner: Uuid,
        water: &ComponentWater,
        light: Option<&WaterLight>,
    ) -> Option<WaterDraw> {
        let path = water.path();
        let profile = water.bank_profile();
        let key = mesh_key(&path, &profile);
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
                    seen: true,
                },
            );
        }
        let step = self.step;
        let cached = self.waters.get_mut(&owner)?;
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
    use crate::test_support::{test_scene, TestAssets};
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
        let scene = test_scene();
        let light = WaterLight {
            direction: -Vec3::y(),
            color: [1.0, 0.5, 0.25],
        };
        let mut cache = WaterCache::default();
        cache.begin_frame(scene.id());
        let first = cache.draw(&context, owner, &water, Some(&light)).unwrap();
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
        cache.begin_frame(scene.id());
        water.wave_amplitude = 0.3;
        water.deep_color = Color32::from_rgb(0x20, 0x40, 0x60);
        water.glint_strength = 2.0;
        let same = cache.draw(&context, owner, &water, Some(&light)).unwrap();
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
        let moved = cache.draw(&context, owner, &water, None).unwrap();
        assert_ne!(moved.mesh.id(), first.mesh.id());
        water.obstacles.push(WaterObstacle {
            position: vec2(0.0, 0.0),
            radius: 1.0,
        });
        let ringed = cache.draw(&context, owner, &water, None).unwrap();
        assert_ne!(ringed.mesh.id(), moved.mesh.id());
        assert_eq!(
            variable("glint_color"),
            bytes(ShaderVariableValue::Color([0.0, 0.0, 0.0, 1.0])),
            "no light, no glints"
        );
        water.bank_height = 0.0;
        water.bank_depth = 0.0;
        assert!(cache
            .draw(&context, owner, &water, None)
            .unwrap()
            .banks
            .is_none());
        water.points.truncate(1);
        assert!(cache.draw(&context, owner, &water, None).is_none());
        cache.end_frame();

        cache.begin_frame(scene.id());
        cache.end_frame();
        assert!(cache.waters.is_empty());
    }
}
