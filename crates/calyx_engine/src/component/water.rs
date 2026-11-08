use crate as calyx_engine;
use crate::assets::material::Material;
use crate::assets::AssetRef;
use crate::component::{Component, ReflectComponent};
use crate::math::{BankProfile, WaterCircle, WaterPath, WaterPathPoint};
use crate::reflect::{Reflect, ReflectDefault};
use crate::render::Gizmos;
use crate::scene::{GameObject, Scene};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use egui::Color32;
use nalgebra_glm::{vec2, vec3, Vec2, Vec3, Vec4};
use serde::{Deserialize, Serialize};

/// Stylized low-poly water: a river along a smooth centreline through
/// `points`, as wide as each point says, whose faceted surface rolls with
/// waves running downstream.
///
/// The surface lies on its owner's local Y = 0, cut into irregular facets
/// about `cell_size` across (see [`WaterPath`]), and is drawn through the
/// ordinary mesh and material path, opaque, into the G-buffer: it is lit,
/// shadowed and fogged like the ground, and casts no shadow itself. Every
/// displacement happens on the GPU: the mesh is built once, and the vertex
/// shader rolls layered waves along the flow, keeping each facet flat with
/// its own normal so the light flickers across the facets as they move.
///
/// Each facet takes one colour, `shallow_color` at the shore running to
/// `deep_color` over `shore_distance`. Foam is drawn per pixel over it, in
/// hard-edged shapes: a broken band about `foam_width` wide along both
/// shores, patches drifting downstream, and rings around each of
/// `obstacles`, all moving with the flow. The scene's brightest directional
/// light glints off the facets that turn towards its mirror direction, as
/// HDR emission of `glint_strength`, so bloom halos them.
///
/// With a `bank_material`, the water also draws its banks: a wall along each
/// shore from below the water up to `bank_height`, then a flat lip, carved
/// along the same curve.
///
/// Colours are sRGB, entered as their hex, and converted to linear as the
/// surface is drawn.
#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect, Clone, Debug, PartialEq)]
#[uuid = "7b0e2f61-3c4d-4a8e-9f15-2d6c8a1e5b93"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Water", category = "Rendering")]
#[serde(default)]
pub struct ComponentWater {
    /// The centreline's control points, from upstream to downstream, in
    /// local units. The water flows from the first to the last.
    pub points: Vec<WaterPoint>,
    /// About how long and wide each cell of two facets is, in local units.
    #[reflect_attr(min = 0.05, speed = 0.01)]
    pub cell_size: f32,
    /// Whether the surface is shaded as flat facets or as smooth, rounded
    /// waves.
    pub shading: WaterShading,
    /// What stands in the water, which the foam rings.
    pub obstacles: Vec<WaterObstacle>,
    /// Height of the wave crests above the rest surface, in local units.
    #[reflect_attr(min = 0.0, speed = 0.005)]
    pub wave_amplitude: f32,
    /// Distance between crests of the main wave, in local units. Shorter
    /// waves and a rolling swell layer over it.
    #[reflect_attr(min = 0.1, speed = 0.05)]
    pub wave_length: f32,
    /// How fast the water flows, in local units per second: the waves roll
    /// and the foam drifts downstream at about this speed.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub flow_speed: f32,
    /// Colour of the water `shore_distance` or more from the shore, sRGB.
    pub deep_color: Color32,
    /// Colour of the water at the shore, sRGB.
    pub shallow_color: Color32,
    /// Colour of the foam, sRGB.
    pub foam_color: Color32,
    /// Distance from the shore over which the colour runs from shallow to
    /// deep, in local units.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub shore_distance: f32,
    /// Width of the foam band along the shore, in local units; it breaks
    /// and swells around this.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub foam_width: f32,
    /// How much of the open water drifting foam patches cover, from 0 (none)
    /// to 1.
    #[reflect_attr(min = 0.0, max = 1.0, speed = 0.005)]
    pub foam_patches: f32,
    /// Width of the foam ring around each obstacle, in local units.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub obstacle_foam: f32,
    /// How much the brightness of neighbouring facets differs, from 0 (none)
    /// to 1 (each facet up to twice or none as bright).
    #[reflect_attr(min = 0.0, max = 1.0, speed = 0.005)]
    pub facet_variation: f32,
    /// Surface roughness: lower is glossier. The clay ground is 0.95 to 1.
    #[reflect_attr(min = 0.0, max = 1.0, speed = 0.01)]
    pub roughness: f32,
    /// Strength of the glints of the brightest directional light, as HDR
    /// emission times the light's colour. 0 draws none.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub glint_strength: f32,
    /// Which ink outline the post-processing Outline effect draws around the
    /// water: 0 for none, `N` for the effect's style `N - 1`, as a
    /// material's `outline_style`. At an edge the effect takes the nearer
    /// surface's style, so the banks, not the water, ink the shore.
    #[reflect_attr(min = 0, max = 255)]
    pub outline_style: u32,
    /// Material of the banks. None draws no banks.
    pub bank_material: AssetRef<Material>,
    /// Height of the bank top above the water's rest surface.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub bank_height: f32,
    /// Depth of the bank wall's foot below the rest surface.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub bank_depth: f32,
    /// Horizontal run of the bank wall from its foot to its top: 0 is a
    /// sheer wall.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub bank_run: f32,
    /// Width of the flat lip along the top of each bank.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub bank_lip: f32,
}

/// How a water surface is shaded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "5e2b7c91-0d34-4f6a-b8e2-7a19c4d3f058"]
pub enum WaterShading {
    /// Low-poly: each facet flat, with its own normal and one colour, so the
    /// light flickers from facet to facet as the waves roll; glints flash
    /// whole facets.
    #[default]
    Faceted,
    /// Smooth: normals taken from the waves' slope at each vertex and
    /// interpolated across the facets, so the waves read round; colour and
    /// glints vary per pixel.
    Smooth,
}

impl WaterShading {
    /// The value the water shader's `shading` takes.
    pub fn shader_value(self) -> u32 {
        match self {
            WaterShading::Faceted => 0,
            WaterShading::Smooth => 1,
        }
    }
}

/// One control point of a river's centreline.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "0f4d2a4e-61b7-4c2c-9a0b-5f3e8d7c1b26"]
#[reflect(Default)]
#[serde(default)]
pub struct WaterPoint {
    /// Local X and Z of the centreline.
    #[reflect_attr(speed = 0.1)]
    pub position: Vec2,
    /// Width of the water from shore to shore here.
    #[reflect_attr(min = 0.1, speed = 0.05)]
    pub width: f32,
}

impl Default for WaterPoint {
    fn default() -> Self {
        Self {
            position: Vec2::zeros(),
            width: 6.0,
        }
    }
}

/// Something standing in the water: a circle in local X and Z.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "a83c9e15-2b4f-4d6a-8e71-c90f3b5d2a47"]
#[reflect(Default)]
#[serde(default)]
pub struct WaterObstacle {
    #[reflect_attr(speed = 0.1)]
    pub position: Vec2,
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub radius: f32,
}

impl Default for WaterObstacle {
    fn default() -> Self {
        Self {
            position: Vec2::zeros(),
            radius: 1.0,
        }
    }
}

impl Default for ComponentWater {
    fn default() -> Self {
        let point = |x: f32, z: f32, width: f32| WaterPoint {
            position: vec2(x, z),
            width,
        };
        Self {
            points: vec![
                point(0.0, -15.0, 6.0),
                point(3.0, -5.0, 7.0),
                point(-3.0, 5.0, 6.0),
                point(0.0, 15.0, 7.0),
            ],
            cell_size: 0.8,
            shading: WaterShading::Faceted,
            obstacles: Vec::new(),
            wave_amplitude: 0.22,
            wave_length: 5.0,
            flow_speed: 1.2,
            // Sunny Battlefield's water (docs/design/art/tank-game-direction.md).
            deep_color: Color32::from_rgb(0x2F, 0x7C, 0xA0),
            shallow_color: Color32::from_rgb(0x5D, 0xB4, 0xBF),
            foam_color: Color32::from_rgb(0xE6, 0xF4, 0xEE),
            shore_distance: 2.5,
            foam_width: 0.45,
            foam_patches: 0.35,
            obstacle_foam: 0.45,
            facet_variation: 0.1,
            roughness: 0.3,
            glint_strength: 2.5,
            outline_style: 0,
            bank_material: Default::default(),
            bank_height: 0.6,
            bank_depth: 0.6,
            bank_run: 0.4,
            bank_lip: 0.5,
        }
    }
}

impl Component for ComponentWater {
    /// The scene renderer skips disabled water.
    fn can_disable(&self) -> bool {
        true
    }

    /// The centreline and both shores while the water is selected: the
    /// shores usually sit against the banks.
    fn draw_gizmos(&self, scene: &Scene, game_object: GameObject, gizmos: &mut Gizmos) {
        if gizmos.selected_object() != Some(scene.uuid(game_object)) {
            return;
        }
        let Some(sections) = self.path().sections() else {
            return;
        };
        let world = scene.world_transform(game_object).matrix();
        let at = |p: Vec2| -> Vec3 { (world * vec3(p.x, 0.0, p.y).push(1.0)).xyz() };
        gizmos.set_color(&Vec4::new(0.4, 0.8, 1.0, 1.0));
        for pair in sections.windows(2) {
            for offset in [-1.0, 0.0, 1.0] {
                let a = pair[0].center + pair[0].normal * pair[0].half_width * offset;
                let b = pair[1].center + pair[1].normal * pair[1].half_width * offset;
                gizmos.line(&at(a), &at(b));
            }
        }
        for point in &self.points {
            gizmos.point(&at(point.position));
        }
    }
}

impl ComponentWater {
    /// The river's centreline, cells and obstacles.
    pub fn path(&self) -> WaterPath {
        WaterPath {
            points: self
                .points
                .iter()
                .map(|point| WaterPathPoint {
                    position: point.position,
                    width: point.width,
                })
                .collect(),
            cell_size: self.cell_size,
            obstacles: self
                .obstacles
                .iter()
                .map(|obstacle| WaterCircle {
                    center: obstacle.position,
                    radius: obstacle.radius,
                })
                .collect(),
        }
    }

    /// The banks' cross-section.
    pub fn bank_profile(&self) -> BankProfile {
        BankProfile {
            height: self.bank_height,
            depth: self.bank_depth,
            run: self.bank_run,
            lip: self.bank_lip,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reflect::TypeInfo;
    use crate::scene::SceneData;
    use crate::test_support::test_registries;

    #[test]
    fn water_is_registered_and_round_trips_through_a_scene() {
        let registries = test_registries();
        assert!(registries
            .components
            .read()
            .component(ComponentWater::type_uuid())
            .is_some());
        let mut scene = registries.scene();
        let owner = scene.create(None, None);
        let water = ComponentWater {
            points: vec![
                WaterPoint {
                    position: vec2(1.0, 2.0),
                    width: 3.0,
                },
                WaterPoint {
                    position: vec2(4.0, 9.0),
                    width: 5.5,
                },
            ],
            cell_size: 0.5,
            shading: WaterShading::Smooth,
            obstacles: vec![WaterObstacle {
                position: vec2(2.0, 4.0),
                radius: 0.75,
            }],
            wave_amplitude: 0.2,
            wave_length: 4.5,
            flow_speed: 0.4,
            deep_color: Color32::from_rgb(1, 2, 3),
            shallow_color: Color32::from_rgb(4, 5, 6),
            foam_color: Color32::from_rgb(7, 8, 9),
            shore_distance: 2.5,
            foam_width: 0.75,
            foam_patches: 0.6,
            obstacle_foam: 0.3,
            facet_variation: 0.3,
            roughness: 0.45,
            glint_strength: 4.0,
            outline_style: 2,
            bank_material: AssetRef::from_id(uuid::Uuid::new_v4()),
            bank_height: 0.7,
            bank_depth: 0.5,
            bank_run: 0.2,
            bank_lip: 0.9,
        };
        scene.add_component(owner, water.clone());
        let data: SceneData = (&scene).into();
        let json = serde_json::to_value(&data).unwrap();
        let restored: SceneData = serde_json::from_value(json).unwrap();
        let value = &restored.components[&scene.uuid(owner)][&ComponentWater::type_uuid()];
        let restored: ComponentWater = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(restored, water);
        // A field a saved scene lacks takes its default.
        let partial: ComponentWater =
            serde_json::from_value(serde_json::json!({ "cell_size": 3.0 })).unwrap();
        assert_eq!(
            partial,
            ComponentWater {
                cell_size: 3.0,
                ..Default::default()
            }
        );
        // The default river bends and meshes.
        assert!(ComponentWater::default().path().geometry().is_some());
    }

    #[test]
    fn every_field_is_reflected_for_the_inspector() {
        let registries = test_registries();
        let types = registries.types.read();
        let Some(TypeInfo::Struct(info)) = types.type_info::<ComponentWater>() else {
            panic!("the water is a reflected struct");
        };
        let names: Vec<&str> = info.fields.keys().copied().collect();
        assert_eq!(
            names,
            [
                "points",
                "cell_size",
                "shading",
                "obstacles",
                "wave_amplitude",
                "wave_length",
                "flow_speed",
                "deep_color",
                "shallow_color",
                "foam_color",
                "shore_distance",
                "foam_width",
                "foam_patches",
                "obstacle_foam",
                "facet_variation",
                "roughness",
                "glint_strength",
                "outline_style",
                "bank_material",
                "bank_height",
                "bank_depth",
                "bank_run",
                "bank_lip",
            ]
        );
        let Some(TypeInfo::Struct(point)) = types.type_info::<WaterPoint>() else {
            panic!("a point is a reflected struct");
        };
        assert!(point.field("position").is_some() && point.field("width").is_some());
        // The inspector edits a field through its reflection.
        let mut water = ComponentWater::default();
        info.field("wave_amplitude")
            .unwrap()
            .set(water.as_reflect_mut(), 0.5f32)
            .unwrap();
        info.field("deep_color")
            .unwrap()
            .set(water.as_reflect_mut(), Color32::RED)
            .unwrap();
        let points = info
            .field("points")
            .unwrap()
            .get_reflect_mut(water.as_reflect_mut())
            .unwrap();
        assert_eq!(points.as_list().unwrap().len(), 4);
        assert_eq!(water.wave_amplitude, 0.5);
        assert_eq!(water.deep_color, Color32::RED);
    }
}
