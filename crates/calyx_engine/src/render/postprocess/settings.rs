//! Post-processing settings, gathered from the effect components beside the
//! global volume that selects them, with the local volumes around the render
//! camera blended over them.

use super::blend::{blend_effect, BlendEffect};
use crate::component::{
    Component, ComponentAmbientOcclusion, ComponentAntiAliasing, ComponentBloom,
    ComponentColorGrading, ComponentDepthOfField, ComponentGlobalPostProcessVolume,
    ComponentLocalPostProcessVolume, ComponentTonemap, ComponentVignette,
};
use crate::scene::{GameObject, Scene};
use legion::storage::ComponentTypeId;
use legion::{Entity, IntoQuery};
use nalgebra_glm::Vec3;
use uuid::Uuid;

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

/// The enabled effect components on one volume's game object, each `None`
/// when the object has no enabled component for it.
///
/// Tone mapping is optional here too: a local volume without a tone-mapping
/// component leaves the exposure below it alone.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PostProcessEffects {
    /// Tone mapping.
    pub tonemapping: Option<ComponentTonemap>,
    /// Screen-space ambient occlusion.
    pub ambient_occlusion: Option<ComponentAmbientOcclusion>,
    /// Depth of field.
    pub depth_of_field: Option<ComponentDepthOfField>,
    /// Bloom.
    pub bloom: Option<ComponentBloom>,
    /// Colour grading.
    pub color_grading: Option<ComponentColorGrading>,
    /// Vignette.
    pub vignette: Option<ComponentVignette>,
    /// Anti-aliasing (FXAA).
    pub anti_aliasing: Option<ComponentAntiAliasing>,
}

impl PostProcessEffects {
    /// The enabled effect components on `game_object`. A missing or disabled
    /// component is `None`.
    pub fn from_game_object(scene: &Scene, game_object: GameObject) -> Self {
        Self {
            tonemapping: effect(scene, game_object),
            ambient_occlusion: effect(scene, game_object),
            depth_of_field: effect(scene, game_object),
            bloom: effect(scene, game_object),
            color_grading: effect(scene, game_object),
            vignette: effect(scene, game_object),
            anti_aliasing: effect(scene, game_object),
        }
    }
}

impl From<PostProcessEffects> for PostProcessSettings {
    /// The settings `effects` describe on their own: tone mapping falls back
    /// to its defaults.
    fn from(effects: PostProcessEffects) -> Self {
        Self {
            tonemapping: effects.tonemapping.unwrap_or_default(),
            ambient_occlusion: effects.ambient_occlusion,
            depth_of_field: effects.depth_of_field,
            bloom: effects.bloom,
            color_grading: effects.color_grading,
            vignette: effects.vignette,
            anti_aliasing: effects.anti_aliasing,
        }
    }
}

/// The items of `volumes`, `(priority, object id, item)` triples, in the
/// order local volumes apply: ascending priority, then ascending object id,
/// so tied volumes keep one order from frame to frame. `tie` is the lowest
/// priority two of them share, if any.
pub(crate) fn order_local_volumes<T>(
    volumes: impl IntoIterator<Item = (i32, Uuid, T)>,
) -> LocalVolumeOrder<T> {
    let mut volumes: Vec<_> = volumes.into_iter().collect();
    volumes.sort_by_key(|(priority, id, _)| (*priority, *id));
    let tie = volumes
        .windows(2)
        .find(|pair| pair[0].0 == pair[1].0)
        .map(|pair| pair[0].0);
    LocalVolumeOrder {
        ordered: volumes.into_iter().map(|(_, _, item)| item).collect(),
        tie,
    }
}

/// The items [`order_local_volumes`] ordered, and the lowest priority
/// several of them share.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LocalVolumeOrder<T> {
    pub ordered: Vec<T>,
    pub tie: Option<i32>,
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
    /// The settings `scene` renders with from a camera at `camera_position`,
    /// in world space: those of [`Self::selected_volume`], or the defaults,
    /// tone mapping alone, when no global volume is enabled, with each of
    /// [`Self::local_volumes`] blended over them in turn.
    pub fn resolve(scene: &Scene, camera_position: &Vec3) -> Self {
        let global = Self::selected_volume(scene)
            .map(|volume| Self::from_game_object(scene, volume))
            .unwrap_or_default();
        Self::local_volumes(scene, camera_position)
            .into_iter()
            .fold(global, |settings, (volume, factor)| {
                settings.blend(&PostProcessEffects::from_game_object(scene, volume), factor)
            })
    }

    /// The enabled local volumes of `scene` that contribute at
    /// `camera_position`, with their blend factors, in the order they apply:
    /// ascending priority, ties in ascending object id with a warning once
    /// per priority. A volume whose factor is zero is left out.
    pub fn local_volumes(scene: &Scene, camera_position: &Vec3) -> Vec<(GameObject, f32)> {
        let mut query = <(Entity, &ComponentLocalPostProcessVolume)>::query();
        let contributing = query
            .iter(scene.world())
            .filter(|(entity, _)| {
                scene.is_type_enabled(
                    **entity,
                    ComponentTypeId::of::<ComponentLocalPostProcessVolume>(),
                )
            })
            .filter_map(|(entity, volume)| {
                let game_object = scene.game_object_from_entity(*entity)?;
                let transform = scene.world_transform(game_object);
                let factor = volume.blend_factor(&transform, camera_position);
                (factor > 0.0).then(|| {
                    (
                        volume.priority,
                        scene.uuid(game_object),
                        (game_object, factor),
                    )
                })
            });
        let order = order_local_volumes(contributing);
        if let Some(priority) = order.tie {
            crate::logging::warn_once(format!("local-post-process-volume-tie:{priority}"), || {
                format!(
                    "Several local post-process volumes around the camera share priority \
                     {priority}; applying them in object id order"
                )
            });
        }
        order.ordered
    }

    /// `self` with a volume's `effects` blended over it by `factor`, from 0
    /// (unchanged) to 1. Each effect the volume has blends field by field;
    /// one it lacks keeps its value here; one that is off here fades in from
    /// its neutral values, or switches on at 0.5 when it has none (FXAA).
    /// See [`BlendEffect`].
    pub fn blend(&self, effects: &PostProcessEffects, factor: f32) -> Self {
        let factor = factor.clamp(0.0, 1.0);
        Self {
            tonemapping: effects.tonemapping.map_or(self.tonemapping, |tonemap| {
                self.tonemapping.blend(&tonemap, factor)
            }),
            ambient_occlusion: blend_effect(
                self.ambient_occlusion,
                effects.ambient_occlusion,
                factor,
            ),
            depth_of_field: blend_effect(self.depth_of_field, effects.depth_of_field, factor),
            bloom: blend_effect(self.bloom, effects.bloom, factor),
            color_grading: blend_effect(self.color_grading, effects.color_grading, factor),
            vignette: blend_effect(self.vignette, effects.vignette, factor),
            anti_aliasing: blend_effect(self.anti_aliasing, effects.anti_aliasing, factor),
        }
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
    ///
    /// It reads [`PostProcessEffects::from_game_object`], the one path that
    /// gathers a volume's effects, global or local.
    pub fn from_game_object(scene: &Scene, game_object: GameObject) -> Self {
        PostProcessEffects::from_game_object(scene, game_object).into()
    }
}
