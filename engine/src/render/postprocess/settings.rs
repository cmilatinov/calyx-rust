//! Post-processing settings and the global volume that selects them.

use crate as engine;
use crate::component::ComponentGlobalPostProcessVolume;
use crate::reflect::{Reflect, ReflectDefault};
use crate::scene::Scene;
use crate::utils::TypeUuid;
use legion::IntoQuery;
use serde::{Deserialize, Serialize};

/// Everything the post-processing chain reads for one frame.
///
/// Effects added later become optional sub-structs, so scenes saved before
/// them still load with those effects off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "ee22c2ed-999e-400e-b348-64abf7294b9c"]
#[reflect(Default)]
#[serde(default)]
pub struct PostProcessSettings {
    /// Tone mapping, the display transform. It always runs.
    pub tonemapping: TonemapSettings,
    /// Screen-space anti-aliasing, after tone mapping. Off by default.
    pub anti_aliasing: super::AntiAliasingSettings,
}

/// Tone-mapping curve and exposure.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "9a7db307-2c75-4b38-ae50-cea01d7f32af"]
#[reflect(Default)]
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

/// The settings [`select_volume`] picked, and the priority several enabled
/// volumes tied at when it had to pick the first of them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct VolumeSelection {
    pub settings: PostProcessSettings,
    pub tie: Option<i32>,
}

/// Picks the enabled volume with the highest priority, the first found on a
/// tie, or the default settings when no volume is enabled.
pub(crate) fn select_volume<'a>(
    volumes: impl IntoIterator<Item = &'a ComponentGlobalPostProcessVolume>,
) -> VolumeSelection {
    let mut selected: Option<&ComponentGlobalPostProcessVolume> = None;
    let mut tie = None;
    for volume in volumes.into_iter().filter(|volume| volume.enabled) {
        match selected {
            Some(best) if volume.priority < best.priority => {}
            Some(best) if volume.priority == best.priority => tie = Some(best.priority),
            _ => {
                selected = Some(volume);
                tie = None;
            }
        }
    }
    VolumeSelection {
        settings: selected.map(|volume| volume.settings).unwrap_or_default(),
        tie,
    }
}

impl PostProcessSettings {
    /// The settings `scene`'s global volumes select, as [`select_volume`]
    /// picks them. Warns once per priority on a tie.
    pub fn resolve(scene: &Scene) -> Self {
        let mut query = <&ComponentGlobalPostProcessVolume>::query();
        let selection = select_volume(query.iter(scene.world()));
        if let Some(priority) = selection.tie {
            crate::logging::warn_once(format!("post-process-volume-tie:{priority}"), || {
                format!(
                    "Several enabled global post-process volumes share priority {priority}; \
                     using the first found"
                )
            });
        }
        selection.settings
    }
}
