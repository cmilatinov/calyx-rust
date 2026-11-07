use crate as calyx_engine;
use crate::assets::material::Material;
use crate::assets::texture::Texture;
use crate::assets::AssetRef;
use crate::component::{Component, ReflectComponent};
use crate::context::ReadOnlyRegistryContext;
use crate::math::{Heightfield, HeightfieldExtent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::scene::{GameObject, Scene};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use legion::{Entity, IntoQuery};
use nalgebra_glm as glm;
use nalgebra_glm::{vec4, Mat4, Vec2, Vec3};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// A ground surface shaped by a heightmap texture, centered on its owner.
///
/// The heightmap's brightness lifts the surface from local Y = 0 (black) to
/// `height_scale` (white) over a local XZ rectangle of `size`. Its top edge
/// lies on the +Z side, so the image reads unflipped seen from above. One
/// vertex is drawn per heightmap pixel, up to
/// [`MAX_HEIGHTFIELD_SAMPLES`](crate::math::MAX_HEIGHTFIELD_SAMPLES) per side,
/// through the ordinary mesh path: the material shades it in the deferred
/// pass, it casts shadows when `cast_shadows` is set and always receives
/// them. Its texture coordinates span the surface once.
///
/// Gameplay code asks for the ground under a world position with
/// [`ComponentTerrain::sample`] or [`ComponentTerrain::sample_scene`].
#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "340f9d11-c4eb-43db-9844-fd5dfa5be321"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Terrain", category = "Rendering")]
#[serde(default)]
pub struct ComponentTerrain {
    /// Grayscale height image, read from its file at full precision: a
    /// 16-bit PNG keeps 65536 levels. A color image's brightness is used.
    pub heightmap: AssetRef<Texture>,
    /// Material the surface is drawn with.
    pub material: AssetRef<Material>,
    /// Width (X) and depth (Z) of the surface, in local units.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub size: Vec2,
    /// Local height of a white heightmap pixel; black lies on Y = 0.
    #[reflect_attr(speed = 0.1)]
    pub height_scale: f32,
    /// Whether the surface draws into the shadow maps of shadowed lights.
    pub cast_shadows: bool,
}

impl Default for ComponentTerrain {
    fn default() -> Self {
        Self {
            heightmap: Default::default(),
            material: Default::default(),
            size: Vec2::new(100.0, 100.0),
            height_scale: 10.0,
            cast_shadows: true,
        }
    }
}

impl Component for ComponentTerrain {
    /// The scene renderer skips a disabled terrain. Sampling does not: a
    /// disabled terrain still answers [`ComponentTerrain::sample`], and only
    /// [`ComponentTerrain::sample_scene`] passes it over.
    fn can_disable(&self) -> bool {
        true
    }
}

/// The ground of a terrain under one world position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerrainSample {
    /// World Y of the drawn surface.
    pub height: f32,
    /// Unit world-space surface normal, interpolated like the drawn
    /// surface's shading normal.
    pub normal: Vec3,
}

impl ComponentTerrain {
    /// How the heightmap is laid on the surface.
    pub fn extent(&self) -> HeightfieldExtent {
        HeightfieldExtent {
            size: self.size,
            height_scale: self.height_scale,
        }
    }

    /// The heightmap's heights, loading the texture if it is not loaded yet.
    /// `None` when the texture is missing or not read from an image file.
    pub fn heightfield(&self, registries: &ReadOnlyRegistryContext) -> Option<Arc<Heightfield>> {
        self.heightmap.get_ref(registries)?.read().heightfield()
    }

    /// The ground of `terrain`'s terrain under world position `point` (X, Z):
    /// the height where a vertical line through it meets the drawn surface,
    /// and the surface normal there. `None` when the object has no terrain,
    /// its heightmap is unusable, or the point is outside its rectangle.
    ///
    /// The terrain is expected upright, turned only around Y, as the surface
    /// is meant to be; under any other rotation the point is taken straight
    /// down the terrain's own Y axis.
    pub fn sample(scene: &Scene, terrain: GameObject, point: Vec2) -> Option<TerrainSample> {
        let (extent, heightfield) = scene
            .read_component::<ComponentTerrain, _, _>(terrain, |c| {
                Some((c.extent(), c.heightfield(scene.registries())?))
            })
            .flatten()?;
        let world = scene.world_transform(terrain).matrix();
        sample_at(&world, &heightfield, &extent, point)
    }

    /// The highest ground of every enabled terrain in `scene` under world
    /// position `point` (X, Z); see [`Self::sample`]. `None` when no terrain
    /// covers it.
    pub fn sample_scene(scene: &Scene, point: Vec2) -> Option<TerrainSample> {
        let mut query = <(Entity, &ComponentTerrain)>::query();
        query
            .iter(scene.world())
            .filter_map(|(entity, _)| scene.game_object_from_entity(*entity))
            .filter(|&terrain| scene.is_component_enabled::<ComponentTerrain>(terrain))
            .filter_map(|terrain| Self::sample(scene, terrain, point))
            .max_by(|a, b| a.height.total_cmp(&b.height))
    }
}

/// The ground under world `point` of a terrain whose owner has world matrix
/// `world`.
fn sample_at(
    world: &Mat4,
    heightfield: &Heightfield,
    extent: &HeightfieldExtent,
    point: Vec2,
) -> Option<TerrainSample> {
    let inverse = world.try_inverse()?;
    let local = inverse * vec4(point.x, 0.0, point.y, 1.0);
    let sample = heightfield.sample(extent, glm::vec2(local.x, local.z))?;
    let surface = world * vec4(local.x, sample.height, local.z, 1.0);
    let normal = inverse.transpose() * vec4(sample.normal.x, sample.normal.y, sample.normal.z, 0.0);
    Some(TerrainSample {
        height: surface.y,
        normal: glm::normalize(&normal.xyz()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Transform;
    use crate::scene::SceneData;
    use crate::test_support::{test_registries, TestAssets};
    use approx::assert_abs_diff_eq;
    use image::{ImageBuffer, Luma};
    use nalgebra::UnitQuaternion;
    use nalgebra_glm::{vec2, vec3};

    #[test]
    fn terrain_is_registered_and_round_trips_through_a_scene() {
        let registries = test_registries();
        assert!(registries
            .components
            .read()
            .component(ComponentTerrain::type_uuid())
            .is_some());
        let mut scene = registries.scene();
        let owner = scene.create(None, None);
        let heightmap = uuid::Uuid::new_v4();
        scene.add_component(
            owner,
            ComponentTerrain {
                heightmap: AssetRef::from_id(heightmap),
                size: vec2(64.0, 32.0),
                height_scale: 7.5,
                cast_shadows: false,
                ..Default::default()
            },
        );
        let data: SceneData = (&scene).into();
        let json = serde_json::to_value(&data).unwrap();
        let restored: SceneData = serde_json::from_value(json).unwrap();
        let value = &restored.components[&scene.uuid(owner)][&ComponentTerrain::type_uuid()];
        let terrain: ComponentTerrain = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(terrain.heightmap, AssetRef::<Texture>::from_id(heightmap));
        assert_eq!(terrain.size, vec2(64.0, 32.0));
        assert_eq!(terrain.height_scale, 7.5);
        assert!(!terrain.cast_shadows);
    }

    #[test]
    fn world_samples_follow_the_owner_transform() {
        // A ramp rising 1 per column along local +X over 3 columns.
        let field = Heightfield::new(3, 2, vec![0.0, 0.5, 1.0, 0.0, 0.5, 1.0]).unwrap();
        let extent = HeightfieldExtent {
            size: vec2(4.0, 4.0),
            height_scale: 2.0,
        };
        // Moved, turned a quarter around Y (local +X to world -Z) and scaled.
        let world = Transform::from_components(
            vec3(10.0, 5.0, -3.0),
            UnitQuaternion::from_axis_angle(&Vec3::y_axis(), std::f32::consts::FRAC_PI_2),
            vec3(2.0, 3.0, 2.0),
        )
        .matrix();
        // World Z -3 - 2 * 1 is local X 1, three quarters up the ramp.
        let sample = sample_at(&world, &field, &extent, vec2(10.0, -5.0)).unwrap();
        assert_abs_diff_eq!(sample.height, 5.0 + 3.0 * 1.5, epsilon = 1e-4);
        // The ramp climbs toward world -Z, so its normal leans toward +Z.
        assert!(sample.normal.z > 0.0 && sample.normal.y > 0.0);
        assert_abs_diff_eq!(sample.normal.x, 0.0, epsilon = 1e-5);
        assert_abs_diff_eq!(sample.normal.norm(), 1.0, epsilon = 1e-5);
        // The local slope of 0.5 per unit, stretched by the scale to 0.5 * 3 / 2.
        assert_abs_diff_eq!(sample.normal.z / sample.normal.y, 0.75, epsilon = 1e-4);
        // Outside the scaled 8 x 8 rectangle.
        assert!(sample_at(&world, &field, &extent, vec2(10.0, -7.5)).is_none());
    }

    #[test]
    fn gameplay_samples_a_terrain_through_its_heightmap_asset() {
        // The texture's upload needs the engine's mip shaders.
        let assets = TestAssets::engine();
        let path = assets.engine_root().join("ramp.png");
        // 16-bit ramp: 0 on the left column, 1 on the right.
        ImageBuffer::from_fn(5, 5, |x, _| Luma([(x * 65535 / 4) as u16]))
            .save(&path)
            .unwrap();
        let registries = assets.registries();
        let heightmap = registries.assets.read().asset_id_from_path(&path).unwrap();
        let mut scene = registries.scene();
        let terrain = scene.create(None, None);
        scene.add_component(
            terrain,
            ComponentTerrain {
                heightmap: AssetRef::from_id(heightmap),
                size: vec2(8.0, 8.0),
                height_scale: 4.0,
                ..Default::default()
            },
        );
        scene.set_transform(terrain, &glm::translation(&vec3(0.0, 1.0, 0.0)));
        let sample = ComponentTerrain::sample(&scene, terrain, vec2(2.0, 0.0)).unwrap();
        assert_abs_diff_eq!(sample.height, 1.0 + 3.0, epsilon = 1e-4);
        assert!(sample.normal.x < 0.0);
        let any = ComponentTerrain::sample_scene(&scene, vec2(2.0, 0.0)).unwrap();
        assert_eq!(any, sample);
        assert!(ComponentTerrain::sample_scene(&scene, vec2(20.0, 0.0)).is_none());

        // A second, higher terrain over part of it wins where both cover.
        let higher = scene.create(None, None);
        scene.add_component(
            higher,
            ComponentTerrain {
                heightmap: AssetRef::from_id(heightmap),
                size: vec2(2.0, 2.0),
                height_scale: 4.0,
                ..Default::default()
            },
        );
        scene.set_transform(higher, &glm::translation(&vec3(2.0, 10.0, 0.0)));
        let top = ComponentTerrain::sample_scene(&scene, vec2(2.0, 0.0)).unwrap();
        assert_abs_diff_eq!(top.height, 10.0 + 2.0, epsilon = 1e-4);
        scene.set_component_enabled::<ComponentTerrain>(higher, false);
        assert_eq!(
            ComponentTerrain::sample_scene(&scene, vec2(2.0, 0.0)),
            Some(sample)
        );
        assert!(ComponentTerrain::sample(&scene, higher, vec2(2.0, 0.0)).is_some());
    }
}
