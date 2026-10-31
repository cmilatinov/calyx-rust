//! Post-processing settings, gathered from the effect components beside the
//! global volume that selects them.

use crate::component::{
    Component, ComponentAmbientOcclusion, ComponentAntiAliasing, ComponentBloom,
    ComponentColorGrading, ComponentDepthOfField, ComponentGlobalPostProcessVolume,
    ComponentTonemap, ComponentVignette,
};
use crate::scene::{GameObject, Scene};
use legion::storage::ComponentTypeId;
use legion::{Entity, IntoQuery};

/// Everything the post-processing chain reads for one frame, gathered from
/// the effect components on the selected volume's game object.
///
/// An effect is `None`, and its pass off, when the volume's object has no
/// enabled component for it. Tone mapping always runs, with its defaults
/// when there is no enabled tone-mapping component.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PostProcessSettings {
    /// Tone mapping, the display transform. It always runs.
    pub tonemapping: ComponentTonemap,
    /// Screen-space ambient occlusion, first in the `Hdr` stage.
    pub ambient_occlusion: Option<ComponentAmbientOcclusion>,
    /// Depth of field, in the `Hdr` stage.
    pub depth_of_field: Option<ComponentDepthOfField>,
    /// Bloom, in the `Hdr` stage.
    pub bloom: Option<ComponentBloom>,
    /// White balance, contrast and saturation, after tone mapping.
    pub color_grading: Option<ComponentColorGrading>,
    /// Darkened screen edges, after tone mapping.
    pub vignette: Option<ComponentVignette>,
    /// Screen-space anti-aliasing (FXAA), after tone mapping.
    pub anti_aliasing: Option<ComponentAntiAliasing>,
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
