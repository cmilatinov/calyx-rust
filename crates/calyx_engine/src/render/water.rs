use crate::assets::material::{Material, ShaderVariableValue};
use crate::assets::mesh::Mesh;
use crate::component::{ComponentDirectionalLight, ComponentWater};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::math::WaterGrid;
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

/// Longest step the waves take in one frame, in seconds, so a stall does not
/// jump them forward.
const MAX_WAVE_STEP: f64 = 0.25;

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

/// How far `phase`, a share of the waves' loop, moves in `seconds` for a
/// main wave `wave_length` long whose crests travel at `wave_speed`,
/// wrapped to `[0, 1)`. Stays put for a wave that is not finite and
/// positive.
pub(crate) fn advance_wave_phase(
    phase: f64,
    seconds: f64,
    wave_speed: f32,
    wave_length: f32,
) -> f64 {
    let rate = wave_speed as f64 / wave_length as f64;
    if !rate.is_finite() || wave_length <= 0.0 {
        return phase;
    }
    (phase + seconds * rate).rem_euclid(1.0)
}

/// A linear RGBA colour for a material, from an sRGB colour.
fn linear(color: Color32) -> ShaderVariableValue {
    ShaderVariableValue::Color(Rgba::from(color).to_array())
}

#[derive(PartialEq)]
struct MeshKey([u32; 4]);

impl MeshKey {
    fn new(grid: &WaterGrid) -> Self {
        Self([
            grid.size.x.to_bits(),
            grid.size.y.to_bits(),
            grid.columns,
            grid.rows,
        ])
    }
}

struct CachedWater {
    key: MeshKey,
    mesh: Option<Ref<Mesh>>,
    material: Ref<Material>,
    /// Where this water's waves are in their loop.
    phase: f64,
    seen: bool,
}

/// The mesh and material of each water a scene renderer draws, one per
/// water owner. The mesh is rebuilt only when the owner's size or cells
/// change; the material, the renderer's own on the water shader, takes the
/// component's colours and waves every frame, so inspector edits show at
/// once.
#[derive(Default)]
pub(super) struct WaterCache {
    scene: Option<SceneId>,
    shader: Option<Ref<Shader>>,
    /// When the last frame was collected, which times the waves.
    last_frame: Option<Instant>,
    /// Seconds since the last frame, at most [`MAX_WAVE_STEP`].
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
            .map_or(0.0, |last| (now - last).as_secs_f64().min(MAX_WAVE_STEP));
        self.last_frame = Some(now);
        for water in self.waters.values_mut() {
            water.seen = false;
        }
    }

    /// The mesh and material of `owner`'s `water`, lit by `light`, or `None`
    /// when its grid is not valid or the water shader did not load.
    pub fn draw(
        &mut self,
        context: &ReadOnlyAssetContext,
        owner: Uuid,
        water: &ComponentWater,
        light: Option<&WaterLight>,
    ) -> Option<(Ref<Mesh>, Ref<Material>)> {
        let grid = water.grid();
        let key = MeshKey::new(&grid);
        if !self.waters.contains_key(&owner) {
            let shader = self.shader(context)?;
            let material = Material::from_shader(context, shader);
            // The draw list batches and looks materials up by id, so each
            // water's material gets its own.
            self.waters.insert(
                owner,
                CachedWater {
                    key: MeshKey([0; 4]),
                    mesh: None,
                    material: Ref::from_id_value(Uuid::new_v4(), material),
                    phase: 0.0,
                    seen: true,
                },
            );
        }
        let cached = self.waters.get_mut(&owner)?;
        if cached.mesh.is_none() || cached.key != key {
            cached.mesh = build_mesh(context, &grid);
            cached.key = key;
        }
        cached.seen = true;
        cached.phase =
            advance_wave_phase(cached.phase, self.step, water.wave_speed, water.wave_length);
        write_material(&mut cached.material.write(), water, cached.phase, light);
        Some((cached.mesh.clone()?, cached.material.clone()))
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

/// Fills the water material from `water`, its waves at `phase`.
fn write_material(
    material: &mut Material,
    water: &ComponentWater,
    phase: f64,
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
        (
            "wave_amplitude",
            ShaderVariableValue::Float(water.wave_amplitude),
        ),
        ("wave_length", ShaderVariableValue::Float(water.wave_length)),
        ("wave_phase", ShaderVariableValue::Float(phase as f32)),
        (
            "shore_distance",
            ShaderVariableValue::Float(water.shore_distance),
        ),
        ("foam_width", ShaderVariableValue::Float(water.foam_width)),
        (
            "facet_variation",
            ShaderVariableValue::Float(water.facet_variation),
        ),
        ("roughness", ShaderVariableValue::Float(water.roughness)),
        (
            "outline_style",
            ShaderVariableValue::Uint(water.outline_style),
        ),
    ] {
        material.set_variable(name, value);
    }
}

fn build_mesh(context: &ReadOnlyAssetContext, grid: &WaterGrid) -> Option<Ref<Mesh>> {
    let geometry = grid.geometry()?;
    let mut mesh = Mesh::new(&context.render_context);
    mesh.tangents = vec![Mesh::DEFAULT_TANGENT; geometry.positions.len()];
    mesh.vertices = geometry.positions;
    mesh.normals = geometry.normals;
    mesh.uvs[0] = geometry.facets;
    mesh.uvs[1] = geometry.next;
    mesh.uvs[2] = geometry.previous;
    mesh.indices = geometry.indices;
    mesh.mark_dirty();
    Some(Ref::from_id_value(Uuid::new_v4(), mesh))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::AssetAccess;
    use crate::test_support::{test_scene, TestAssets};
    use approx::assert_abs_diff_eq;
    use nalgebra_glm::vec2;

    #[test]
    fn the_waves_loop_once_per_wave_length_over_their_speed() {
        // A 6 m wave at 0.8 m/s loops every 7.5 s.
        // Within f32 rounding of the speed, and of a whole loop either side of 0.
        let looped = |phase: f64| phase.min(1.0 - phase);
        assert_abs_diff_eq!(advance_wave_phase(0.0, 3.75, 0.8, 6.0), 0.5, epsilon = 1e-6);
        assert_abs_diff_eq!(
            looped(advance_wave_phase(0.0, 7.5, 0.8, 6.0)),
            0.0,
            epsilon = 1e-6
        );
        assert_abs_diff_eq!(advance_wave_phase(0.9, 1.5, 0.8, 6.0), 0.1, epsilon = 1e-6);
        // Still water, or a wave with no length, stays put.
        assert_eq!(advance_wave_phase(0.3, 1.0, 0.0, 6.0), 0.3);
        assert_eq!(advance_wave_phase(0.3, 1.0, 0.8, 0.0), 0.3);
        assert_eq!(advance_wave_phase(0.3, 1.0, f32::NAN, 6.0), 0.3);
    }

    #[test]
    fn cache_rebuilds_the_mesh_only_for_a_new_grid_and_fills_the_material_each_frame() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let mut water = ComponentWater {
            size: vec2(4.0, 2.0),
            columns: 4,
            rows: 2,
            ..Default::default()
        };
        let owner = Uuid::new_v4();
        let scene = test_scene();
        let light = WaterLight {
            direction: -Vec3::y(),
            color: [1.0, 0.5, 0.25],
        };
        let mut cache = WaterCache::default();
        cache.begin_frame(scene.id());
        let (mesh, material) = cache.draw(&context, owner, &water, Some(&light)).unwrap();
        assert_eq!(mesh.read().vertices.len(), 4 * 2 * 6);
        assert_eq!(mesh.read().indices.len(), 4 * 2 * 6);
        assert_eq!(material.read().shader.id(), {
            let shader: Ref<Shader> = SceneRendererError::load(&context, WATER_SHADER).unwrap();
            shader.id()
        });
        cache.end_frame();

        // Colours and waves reach the same material without a rebuild.
        cache.begin_frame(scene.id());
        water.wave_amplitude = 0.3;
        water.deep_color = Color32::from_rgb(0x20, 0x40, 0x60);
        water.glint_strength = 2.0;
        let (same_mesh, same_material) = cache.draw(&context, owner, &water, Some(&light)).unwrap();
        assert_eq!(same_mesh.id(), mesh.id());
        assert_eq!(same_material.id(), material.id());
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

        // A new size or cell count rebuilds the mesh.
        water.columns = 5;
        let (rebuilt, _) = cache.draw(&context, owner, &water, None).unwrap();
        assert_ne!(rebuilt.id(), mesh.id());
        assert_eq!(rebuilt.read().vertices.len(), 5 * 2 * 6);
        assert_eq!(
            variable("glint_color"),
            bytes(ShaderVariableValue::Color([0.0, 0.0, 0.0, 1.0])),
            "no light, no glints"
        );
        water.size.x = 0.0;
        assert!(cache.draw(&context, owner, &water, None).is_none());
        cache.end_frame();

        cache.begin_frame(scene.id());
        cache.end_frame();
        assert!(cache.waters.is_empty());
    }
}
