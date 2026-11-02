//! Post-processing profile assets: a set of effect settings that any number
//! of post-process volumes reference.
//!
//! A volume's effects are its profile's entries overlaid with the enabled
//! effect components on its own game object; see
//! [`PostProcessEffects::from_game_object`].

use std::path::Path;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate as calyx_engine;
use crate::assets::error::AssetError;
use crate::assets::{Asset, AssetAccess, AssetRegistry, LoadedAsset};
use crate::component::{
    ComponentAmbientOcclusion, ComponentAntiAliasing, ComponentBloom, ComponentColorGrading,
    ComponentDepthOfField, ComponentGlobalPostProcessVolume, ComponentLocalPostProcessVolume,
    ComponentTonemap, ComponentVignette,
};
use crate::context::ReadOnlyAssetContext;
use crate::reflect::Reflect;
use crate::render::postprocess::PostProcessEffects;
use crate::scene::{GameObject, Scene};
use crate::utils::TypeUuid;

/// One effect's settings in a [`PostProcessProfile`], with whether the
/// profile switches the effect on.
///
/// Every field is optional in a saved profile: a missing `enabled` is on and
/// missing settings take the effect's defaults.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, bound(deserialize = "T: Deserialize<'de> + Default"))]
pub struct ProfileEntry<T> {
    /// Whether the effect is on. A disabled entry switches the effect off on
    /// every volume that uses the profile, unless the volume's object holds
    /// an enabled component for it.
    pub enabled: bool,
    /// The effect's settings, the same type as its effect component.
    pub settings: T,
}

impl<T: Default> Default for ProfileEntry<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T> ProfileEntry<T> {
    /// An enabled entry with `settings`.
    pub fn new(settings: T) -> Self {
        Self {
            enabled: true,
            settings,
        }
    }

    /// The settings when the entry is enabled.
    fn effect(&self) -> Option<T>
    where
        T: Copy,
    {
        self.enabled.then_some(self.settings)
    }
}

/// Invokes the macro `$mac` once for each effect field of a
/// [`PostProcessProfile`], with the field's name and its effect component
/// type.
macro_rules! for_each_effect {
    ($mac:ident) => {
        $mac!(tonemapping, ComponentTonemap);
        $mac!(ambient_occlusion, ComponentAmbientOcclusion);
        $mac!(depth_of_field, ComponentDepthOfField);
        $mac!(bloom, ComponentBloom);
        $mac!(color_grading, ComponentColorGrading);
        $mac!(vignette, ComponentVignette);
        $mac!(anti_aliasing, ComponentAntiAliasing);
    };
}

/// A post-processing look saved as an asset (`.cxpostfx`), which any number
/// of global and local volumes reference through their `profile` field.
///
/// It holds an optional entry for each effect. A volume uses an enabled
/// entry unless its game object holds an enabled component for the same
/// effect, which replaces the entry. Profiles do not blend with or layer
/// over each other; local volumes blend the settings each volume resolves
/// to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, TypeUuid)]
#[uuid = "2f2b4d1b-4d61-4751-b2e3-222579256f15"]
#[serde(default)]
pub struct PostProcessProfile {
    /// Tone mapping.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tonemapping: Option<ProfileEntry<ComponentTonemap>>,
    /// Screen-space ambient occlusion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ambient_occlusion: Option<ProfileEntry<ComponentAmbientOcclusion>>,
    /// Depth of field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth_of_field: Option<ProfileEntry<ComponentDepthOfField>>,
    /// Bloom.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bloom: Option<ProfileEntry<ComponentBloom>>,
    /// Colour grading.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_grading: Option<ProfileEntry<ComponentColorGrading>>,
    /// Vignette.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vignette: Option<ProfileEntry<ComponentVignette>>,
    /// Anti-aliasing (FXAA).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anti_aliasing: Option<ProfileEntry<ComponentAntiAliasing>>,
}

/// One entry of a [`PostProcessProfile`], reached by its effect's component
/// type, for the editor.
pub struct ProfileEntryMut<'a> {
    /// Whether the entry is on.
    pub enabled: &'a mut bool,
    /// The effect's settings, as its effect component.
    pub settings: &'a mut dyn Reflect,
}

impl PostProcessProfile {
    /// The type UUIDs of the effect components a profile holds entries for,
    /// in the order the chain runs their passes' stages: tone mapping, then
    /// the `Hdr` effects, then the ones after tone mapping.
    pub fn effect_types() -> [Uuid; 7] {
        [
            ComponentTonemap::type_uuid(),
            ComponentAmbientOcclusion::type_uuid(),
            ComponentDepthOfField::type_uuid(),
            ComponentBloom::type_uuid(),
            ComponentColorGrading::type_uuid(),
            ComponentVignette::type_uuid(),
            ComponentAntiAliasing::type_uuid(),
        ]
    }

    /// The effects the profile switches on: each enabled entry's settings,
    /// and `None` for a disabled or absent one.
    pub fn effects(&self) -> PostProcessEffects {
        PostProcessEffects {
            tonemapping: self.tonemapping.as_ref().and_then(ProfileEntry::effect),
            ambient_occlusion: self
                .ambient_occlusion
                .as_ref()
                .and_then(ProfileEntry::effect),
            depth_of_field: self.depth_of_field.as_ref().and_then(ProfileEntry::effect),
            bloom: self.bloom.as_ref().and_then(ProfileEntry::effect),
            color_grading: self.color_grading.as_ref().and_then(ProfileEntry::effect),
            vignette: self.vignette.as_ref().and_then(ProfileEntry::effect),
            anti_aliasing: self.anti_aliasing.as_ref().and_then(ProfileEntry::effect),
        }
    }

    /// A profile that keeps the look of the volume on `game_object`: its
    /// current profile's entries, if it has one, with each effect component
    /// on the object copied over them. An enabled component replaces its
    /// entry; a disabled one becomes a disabled entry only where the profile
    /// has none, since a disabled component leaves the profile's entry in
    /// effect.
    pub fn from_game_object(scene: &Scene, game_object: GameObject) -> Self {
        let mut profile = Self::of_volume(scene, game_object).unwrap_or_default();
        macro_rules! copy_component {
            ($field:ident, $ty:ty) => {
                if let Some(settings) =
                    scene.read_component::<$ty, _, _>(game_object, |component| *component)
                {
                    let enabled = scene.is_component_enabled::<$ty>(game_object);
                    if enabled || profile.$field.is_none() {
                        profile.$field = Some(ProfileEntry { enabled, settings });
                    }
                }
            };
        }
        for_each_effect!(copy_component);
        profile
    }

    /// The profile the volume on `game_object` references, global or local,
    /// the global volume's when the object has both, or `None` when it has
    /// none or the profile does not load. A profile that does not load is
    /// warned about once.
    pub fn of_volume(scene: &Scene, game_object: GameObject) -> Option<Self> {
        let id = Self::volume_profile_id(scene, game_object)?;
        let loaded = scene
            .registries()
            .assets
            .read()
            .load_by_id::<PostProcessProfile>(id);
        match loaded {
            Ok(profile) => Some(*profile.read()),
            Err(error) => {
                crate::logging::warn_once(format!("post-process-profile:{id}"), || {
                    format!(
                        "Post-process volume on {} uses profile {id}, which does not load: \
                         {error}",
                        scene.name(game_object)
                    )
                });
                None
            }
        }
    }

    /// The id of the profile the volume on `game_object` references, the
    /// global volume's when the object has both, or `None` without one.
    pub fn volume_profile_id(scene: &Scene, game_object: GameObject) -> Option<Uuid> {
        let global = scene
            .read_component::<ComponentGlobalPostProcessVolume, _, _>(game_object, |volume| {
                volume.profile.id()
            });
        let local = || {
            scene.read_component::<ComponentLocalPostProcessVolume, _, _>(game_object, |volume| {
                volume.profile.id()
            })
        };
        global
            .filter(|id| !id.is_nil())
            .or_else(|| local().filter(|id| !id.is_nil()))
    }

    /// The entry for the effect component type `type_uuid`, when the profile
    /// has one.
    pub fn entry_mut(&mut self, type_uuid: Uuid) -> Option<ProfileEntryMut<'_>> {
        macro_rules! entry {
            ($field:ident, $ty:ty) => {
                if type_uuid == <$ty>::type_uuid() {
                    return self.$field.as_mut().map(|entry| ProfileEntryMut {
                        enabled: &mut entry.enabled,
                        settings: &mut entry.settings,
                    });
                }
            };
        }
        for_each_effect!(entry);
        None
    }

    /// Whether the profile has an entry for the effect component type
    /// `type_uuid`.
    pub fn has_entry(&self, type_uuid: Uuid) -> bool {
        macro_rules! has {
            ($field:ident, $ty:ty) => {
                if type_uuid == <$ty>::type_uuid() {
                    return self.$field.is_some();
                }
            };
        }
        for_each_effect!(has);
        false
    }

    /// Adds an enabled entry with the defaults for the effect component type
    /// `type_uuid`, unless the profile has one. Returns whether it added it.
    pub fn add_entry(&mut self, type_uuid: Uuid) -> bool {
        macro_rules! add {
            ($field:ident, $ty:ty) => {
                if type_uuid == <$ty>::type_uuid() {
                    if self.$field.is_some() {
                        return false;
                    }
                    self.$field = Some(ProfileEntry::default());
                    return true;
                }
            };
        }
        for_each_effect!(add);
        false
    }

    /// Removes the entry for the effect component type `type_uuid`. Returns
    /// whether the profile had one.
    pub fn remove_entry(&mut self, type_uuid: Uuid) -> bool {
        macro_rules! remove {
            ($field:ident, $ty:ty) => {
                if type_uuid == <$ty>::type_uuid() {
                    return self.$field.take().is_some();
                }
            };
        }
        for_each_effect!(remove);
        false
    }
}

impl Asset for PostProcessProfile {
    fn asset_name() -> &'static str
    where
        Self: Sized,
    {
        "Post Process Profile"
    }

    fn file_extensions() -> &'static [&'static str]
    where
        Self: Sized,
    {
        &["cxpostfx"]
    }

    fn from_file(
        _assets: &ReadOnlyAssetContext,
        path: &Path,
    ) -> Result<LoadedAsset<Self>, AssetError>
    where
        Self: Sized,
    {
        LoadedAsset::<Self>::from_json_file(path)
    }

    fn to_file(&self, path: &Path) -> Result<(), std::io::Error> {
        AssetRegistry::write_to_file(self, path)
    }
}

#[cfg(test)]
mod tests;
