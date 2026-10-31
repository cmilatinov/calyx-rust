//! Post-processing settings, the effect components they are gathered from,
//! and the global volume that selects them.

use super::{
    AmbientOcclusionSettings, AntiAliasingSettings, BloomSettings, ColorGradingSettings,
    DepthOfFieldSettings, VignetteSettings,
};
use crate as calyx_engine;
use crate::component::{Component, ComponentGlobalPostProcessVolume, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::scene::{GameObject, Scene};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use legion::storage::ComponentTypeId;
use legion::{Entity, IntoQuery};
use serde::{Deserialize, Serialize};

/// Everything the post-processing chain reads for one frame, gathered from
/// the effect components on the selected volume's game object.
///
/// An effect is `None`, and its pass off, when the volume's object has no
/// enabled component for it. Tone mapping always runs, with its defaults
/// when there is no enabled tone-mapping component.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PostProcessSettings {
    /// Tone mapping, the display transform. It always runs.
    pub tonemapping: TonemapSettings,
    /// Screen-space ambient occlusion, first in the `Hdr` stage.
    pub ambient_occlusion: Option<AmbientOcclusionSettings>,
    /// Depth of field, in the `Hdr` stage.
    pub depth_of_field: Option<DepthOfFieldSettings>,
    /// Bloom, in the `Hdr` stage.
    pub bloom: Option<BloomSettings>,
    /// White balance, contrast and saturation, after tone mapping.
    pub color_grading: Option<ColorGradingSettings>,
    /// Darkened screen edges, after tone mapping.
    pub vignette: Option<VignetteSettings>,
    /// Screen-space anti-aliasing (FXAA), after tone mapping.
    pub anti_aliasing: Option<AntiAliasingSettings>,
}

/// Tone-mapping curve and exposure, as an effect component on a
/// post-process volume's game object.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Component, Reflect)]
#[uuid = "68ac12ac-fc9a-489e-b305-363eaf593966"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Tone Mapping", category = "Post Processing")]
#[serde(default)]
pub struct TonemapSettings {
    /// Curve that maps linear HDR color to the display range.
    pub preset: TonemappingPreset,
    /// Exposure in stops: the scene is scaled by `2^exposure_ev` before the
    /// curve.
    #[reflect_attr(speed = 0.05)]
    pub exposure_ev: f32,
}

impl Default for TonemapSettings {
    fn default() -> Self {
        Self {
            preset: TonemappingPreset::Aces,
            exposure_ev: 0.0,
        }
    }
}

impl Component for TonemapSettings {
    /// A disabled tone-mapping component leaves the defaults in effect.
    fn can_disable(&self) -> bool {
        true
    }
}

impl TonemapSettings {
    /// The linear factor the scene is scaled by before the curve.
    pub fn exposure_scale(&self) -> f32 {
        self.exposure_ev.exp2()
    }
}

/// Tone-mapping curves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "a4d6b439-bdc2-4aeb-aaf7-1f14f147255b"]
pub enum TonemappingPreset {
    /// Stephen Hill's fit of the ACES reference rendering and output
    /// transforms.
    #[default]
    Aces,
}

impl TonemappingPreset {
    /// The value the tone-map shader switches on.
    pub(crate) fn shader_index(self) -> u32 {
        match self {
            Self::Aces => 0,
        }
    }
}

/// The item [`select_volume`] picked, and the priority several enabled
/// volumes tied at when it had to pick the first of them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct VolumeSelection<T> {
    pub selected: Option<T>,
    pub tie: Option<i32>,
}

/// Picks the enabled volume with the highest priority, or the first found
/// on a tie, from `(enabled, priority, item)` triples. `selected` is `None`
/// when no volume is enabled.
pub(crate) fn select_volume<T>(
    volumes: impl IntoIterator<Item = (bool, i32, T)>,
) -> VolumeSelection<T> {
    let mut selected: Option<(i32, T)> = None;
    let mut tie = None;
    let enabled = volumes
        .into_iter()
        .filter_map(|(enabled, priority, item)| enabled.then_some((priority, item)));
    for (priority, item) in enabled {
        match &selected {
            Some((best, _)) if priority < *best => {}
            Some((best, _)) if priority == *best => tie = Some(*best),
            _ => {
                selected = Some((priority, item));
                tie = None;
            }
        }
    }
    VolumeSelection {
        selected: selected.map(|(_, item)| item),
        tie,
    }
}

/// `game_object`'s effect component `T`, or `None` when it has none or the
/// scene holds it disabled.
fn effect<T: Component + Copy + 'static>(scene: &Scene, game_object: GameObject) -> Option<T> {
    scene
        .read_component::<T, _, _>(game_object, |effect| *effect)
        .filter(|_| scene.is_component_enabled::<T>(game_object))
}

impl PostProcessSettings {
    /// The settings `scene` renders with: those of [`Self::selected_volume`],
    /// or the defaults, tone mapping alone, when no volume is enabled.
    pub fn resolve(scene: &Scene) -> Self {
        Self::selected_volume(scene)
            .map(|volume| Self::from_game_object(scene, volume))
            .unwrap_or_default()
    }

    /// The game object of `scene`'s enabled global volume with the highest
    /// priority, the first found on a tie, or `None` when no volume is
    /// enabled. Warns once per priority on a tie.
    pub fn selected_volume(scene: &Scene) -> Option<GameObject> {
        let mut query = <(Entity, &ComponentGlobalPostProcessVolume)>::query();
        let selection = select_volume(query.iter(scene.world()).map(|(entity, volume)| {
            let enabled = scene.is_type_enabled(
                *entity,
                ComponentTypeId::of::<ComponentGlobalPostProcessVolume>(),
            );
            (enabled, volume.priority, *entity)
        }));
        if let Some(priority) = selection.tie {
            crate::logging::warn_once(format!("post-process-volume-tie:{priority}"), || {
                format!(
                    "Several enabled global post-process volumes share priority {priority};                      using the first found"
                )
            });
        }
        selection
            .selected
            .and_then(|entity| scene.game_object_from_entity(entity))
    }

    /// The settings the enabled effect components on `game_object` describe.
    /// A missing or disabled effect is off, and tone mapping falls back to
    /// its defaults.
    pub fn from_game_object(scene: &Scene, game_object: GameObject) -> Self {
        Self {
            tonemapping: effect(scene, game_object).unwrap_or_default(),
            ambient_occlusion: effect(scene, game_object),
            depth_of_field: effect(scene, game_object),
            bloom: effect(scene, game_object),
            color_grading: effect(scene, game_object),
            vignette: effect(scene, game_object),
            anti_aliasing: effect(scene, game_object),
        }
    }
}
