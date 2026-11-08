use crate as calyx_engine;
use crate::component::{Component, ReflectComponent};
use crate::math::WaterGrid;
use crate::reflect::{Reflect, ReflectDefault};
use crate::render::Gizmos;
use crate::scene::{GameObject, Scene};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use egui::Color32;
use nalgebra_glm::{vec2, vec3, Vec2, Vec4};
use serde::{Deserialize, Serialize};

/// Stylized low-poly water: a faceted surface over a local XZ rectangle of
/// `size`, centered on its owner, whose vertices bob in a slow wave.
///
/// The surface is cut into `columns` by `rows` cells of two irregular
/// triangles (see [`WaterGrid`]) and drawn through the ordinary mesh and
/// material path, opaque, into the G-buffer: it is lit, shadowed and
/// fogged like the ground, and casts no shadow itself. Each facet is shaded
/// flat with its own normal, so the light catches the facets differently as
/// they move, and takes one colour: `shallow_color` at the shore, the
/// rectangle's outline, running to `deep_color` over `shore_distance`, with
/// `foam_color` on the facets within about `foam_width` of the shore.
///
/// Colours are sRGB, entered as their hex, and converted to linear as the
/// surface is drawn. The scene's brightest directional light glints off the
/// facets that turn towards its mirror direction, as HDR emission of
/// `glint_strength`, so a post-processing volume's bloom halos them.
#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect, Clone, Debug, PartialEq)]
#[uuid = "7b0e2f61-3c4d-4a8e-9f15-2d6c8a1e5b93"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Water", category = "Rendering")]
#[serde(default)]
pub struct ComponentWater {
    /// Width (X) and depth (Z) of the surface, in local units.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub size: Vec2,
    /// Cells along X. A cell is two facets.
    #[reflect_attr(min = 1, max = 512)]
    pub columns: u32,
    /// Cells along Z.
    #[reflect_attr(min = 1, max = 512)]
    pub rows: u32,
    /// Height of the wave crests above the rest surface, in local units.
    #[reflect_attr(min = 0.0, speed = 0.005)]
    pub wave_amplitude: f32,
    /// Distance between crests of the main wave, in local units. Two
    /// smaller waves cross it at fixed fractions of it.
    #[reflect_attr(min = 0.1, speed = 0.05)]
    pub wave_length: f32,
    /// How fast the main wave's crests travel, in local units per second.
    /// The waves loop every `wave_length / wave_speed` seconds.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub wave_speed: f32,
    /// Colour of the water `shore_distance` or more from the shore, sRGB.
    pub deep_color: Color32,
    /// Colour of the water at the shore, sRGB.
    pub shallow_color: Color32,
    /// Colour of the foam along the shore, sRGB.
    pub foam_color: Color32,
    /// Distance from the shore over which the colour runs from shallow to
    /// deep, in local units.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub shore_distance: f32,
    /// Width of the foam line along the shore, in local units. Each facet's
    /// foam edge varies around it, and rises and falls a little with the
    /// wave, so the line is broken rather than ruled.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub foam_width: f32,
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
    /// surface's style, so water sunk below its banks shows only where it
    /// is nearer than what it meets.
    #[reflect_attr(min = 0, max = 255)]
    pub outline_style: u32,
}

impl Default for ComponentWater {
    fn default() -> Self {
        Self {
            size: vec2(20.0, 10.0),
            columns: 20,
            rows: 10,
            wave_amplitude: 0.12,
            wave_length: 6.0,
            wave_speed: 0.8,
            // Sunny Battlefield's water (docs/design/art/tank-game-direction.md).
            deep_color: Color32::from_rgb(0x2F, 0x7C, 0xA0),
            shallow_color: Color32::from_rgb(0x5D, 0xB4, 0xBF),
            foam_color: Color32::from_rgb(0xE6, 0xF4, 0xEE),
            shore_distance: 3.0,
            foam_width: 0.35,
            facet_variation: 0.12,
            roughness: 0.3,
            glint_strength: 2.5,
            outline_style: 0,
        }
    }
}

impl Component for ComponentWater {
    /// The scene renderer skips disabled water.
    fn can_disable(&self) -> bool {
        true
    }

    /// The rectangle's outline, the shore, while the water is selected: its
    /// edges usually sit under the banks.
    fn draw_gizmos(&self, scene: &Scene, game_object: GameObject, gizmos: &mut Gizmos) {
        if gizmos.selected_object() != Some(scene.uuid(game_object)) {
            return;
        }
        let world = scene.world_transform(game_object).matrix();
        let half = self.size * 0.5;
        let corners = [
            vec2(-half.x, -half.y),
            vec2(half.x, -half.y),
            vec2(half.x, half.y),
            vec2(-half.x, half.y),
        ]
        .map(|corner| (world * vec3(corner.x, 0.0, corner.y).push(1.0)).xyz());
        gizmos.set_color(&Vec4::new(0.4, 0.8, 1.0, 1.0));
        for i in 0..4 {
            gizmos.line(&corners[i], &corners[(i + 1) % 4]);
        }
    }
}

impl ComponentWater {
    /// The surface's cells.
    pub fn grid(&self) -> WaterGrid {
        WaterGrid {
            size: self.size,
            columns: self.columns,
            rows: self.rows,
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
            size: vec2(12.0, 40.0),
            columns: 9,
            rows: 31,
            wave_amplitude: 0.2,
            wave_length: 4.5,
            wave_speed: 0.4,
            deep_color: Color32::from_rgb(1, 2, 3),
            shallow_color: Color32::from_rgb(4, 5, 6),
            foam_color: Color32::from_rgb(7, 8, 9),
            shore_distance: 2.5,
            foam_width: 0.75,
            facet_variation: 0.3,
            roughness: 0.45,
            glint_strength: 4.0,
            outline_style: 2,
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
            serde_json::from_value(serde_json::json!({ "columns": 3 })).unwrap();
        assert_eq!(
            partial,
            ComponentWater {
                columns: 3,
                ..Default::default()
            }
        );
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
                "size",
                "columns",
                "rows",
                "wave_amplitude",
                "wave_length",
                "wave_speed",
                "deep_color",
                "shallow_color",
                "foam_color",
                "shore_distance",
                "foam_width",
                "facet_variation",
                "roughness",
                "glint_strength",
                "outline_style",
            ]
        );
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
        assert_eq!(water.wave_amplitude, 0.5);
        assert_eq!(water.deep_color, Color32::RED);
    }
}
