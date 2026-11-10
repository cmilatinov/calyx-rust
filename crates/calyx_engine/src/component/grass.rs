//! Tall grass that sways in the wind, bends away from the things moving
//! through it, and fades around them so they can be seen inside it.

use crate as calyx_engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra_glm::Vec3;
use serde::{Deserialize, Serialize};

/// The most benders the grass shader reads; the rest are ignored.
pub const MAX_GRASS_BENDERS: usize = 16;

/// The scene's tall grass: every mesh drawn with the grass material shader,
/// `shaders/mesh/grass`, sways and bends by the first enabled one of these in
/// the scene. Without one, grass stands still and bends for nothing.
///
/// The shader moves a vertex by its height above its mesh's origin, so a
/// grass model stands on its origin with its blades rising along +Y: the
/// higher a vertex, the further it sways and bends. Each blade is its own
/// mesh, modelled lying flat: its outline in XZ, its length running along
/// -Z from its root at the mesh's origin, its thickness rising along +Y from
/// 0. The shader thins a blade towards its tip from that layout, so the tip
/// ends in a thin edge. Shadows are cast from the grass at rest.
///
/// What moves through the grass is game state: game code hands it to the
/// grass in [`Self::benders`] every frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Component, Reflect)]
#[uuid = "7c4e2a91-5d38-4f0b-b6e1-93a8d27c5f14"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Grass Field", category = "Rendering")]
#[serde(default)]
pub struct ComponentGrassField {
    /// How far a blade's tip sways in the wind, in metres per metre of
    /// height.
    #[reflect_attr(min = 0.0, speed = 0.005)]
    pub sway: f32,
    /// How fast the grass sways, in radians per second.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub sway_speed: f32,
    /// How far a blade next to a bender bends away from it, in metres per
    /// metre of height.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub bend: f32,
    /// How far the grass droops down and outward from its mesh's origin, in
    /// metres per metre of height, growing with the square of the height,
    /// so straight blades bend in a smooth arc. 0 leaves them as modelled.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub droop: f32,
    /// The height, in metres, a blade reaches full sway and bend at.
    #[reflect_attr(min = 0.01, speed = 0.05)]
    pub height: f32,
    /// What moves through the grass this frame, at most
    /// [`MAX_GRASS_BENDERS`]. Runtime state: never saved.
    #[serde(skip)]
    #[reflect_skip]
    pub benders: Vec<GrassBender>,
}

impl Default for ComponentGrassField {
    fn default() -> Self {
        Self {
            sway: 0.06,
            sway_speed: 1.7,
            bend: 0.18,
            droop: 0.0,
            height: 3.4,
            benders: Vec::new(),
        }
    }
}

impl Component for ComponentGrassField {
    /// A disabled field leaves the grass still.
    fn can_disable(&self) -> bool {
        true
    }
}

/// Something moving through the grass.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GrassBender {
    /// Where it stands, in world space; only the ground position counts.
    pub position: Vec3,
    /// How far from it, in metres, the grass bends away.
    pub bend_radius: f32,
    /// How far from it, in metres, the grass fades so it shows through; 0
    /// fades nothing. The grass eases from opaque at the radius to its
    /// least opacity a third of the way in, alpha blended, with no ink.
    pub reveal_radius: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_registries;

    #[test]
    fn grass_field_is_registered_and_saves_no_benders() {
        let registries = test_registries();
        assert!(registries
            .components
            .read()
            .component(ComponentGrassField::type_uuid())
            .is_some());
        let field = ComponentGrassField {
            benders: vec![GrassBender::default()],
            ..Default::default()
        };
        let saved = serde_json::to_value(&field).unwrap();
        assert!(saved.get("benders").is_none(), "{saved}");
        let loaded: ComponentGrassField = serde_json::from_value(saved).unwrap();
        assert!(loaded.benders.is_empty());
        assert_eq!(loaded.sway, field.sway);
    }
}
