//! Post-processing profile assets: a set of effect settings that any number
//! of post-process volumes reference.
//!
//! A volume's effects are its profile's entries overlaid with the enabled
//! effect components on its own game object; see
//! [`PostProcessEffects::from_game_object`].
//!
//! # File format
//!
//! A `.cxpostfx` file is JSON holding a list of entries, each keyed by its
//! effect component's type UUID, with the component's serialized fields as
//! a scene saves them:
//!
//! ```json
//! {
//!   "entries": [
//!     { "type": "7e09858d-fd71-4729-bb90-8ed76c0bda9a", "enabled": true,
//!       "settings": { "threshold": 1.0, "knee": 0.5, "intensity": 0.8, "radius": 6 } }
//!   ]
//! }
//! ```
//!
//! `enabled` defaults to `true` and `settings` to the effect's defaults,
//! field by field. An entry whose type is unknown or not a post-process
//! effect, whose settings do not deserialize, or whose type an earlier entry
//! already has, is skipped with a warning; the rest of the profile loads.

use std::fmt;
use std::io::BufReader;
use std::path::Path;

use serde::{Deserialize, Serialize, Serializer};
use uuid::Uuid;

use crate as calyx_engine;
use crate::assets::error::AssetError;
use crate::assets::{Asset, AssetAccess, AssetRegistry, LoadedAsset};
use crate::class_registry::ComponentRegistry;
use crate::component::{
    ComponentAmbientOcclusion, ComponentAntiAliasing, ComponentBloom, ComponentColorGrading,
    ComponentDepthOfField, ComponentFog, ComponentGlobalPostProcessVolume,
    ComponentLocalPostProcessVolume, ComponentOutline, ComponentTonemap, ComponentVignette,
    PostProcessEffect, ReflectPostProcessEffect,
};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::reflect::type_registry::TypeRegistry;
use crate::reflect::ReflectDefault;
use crate::render::postprocess::PostProcessEffects;
use crate::scene::{GameObject, Scene};
use crate::type_uuids;
use crate::utils::TypeUuid;

/// One effect's settings in a [`PostProcessProfile`], with whether the
/// profile switches the effect on.
pub struct ProfileEntry {
    /// Whether the effect is on. A disabled entry switches the effect off on
    /// every volume that uses the profile, unless the volume's object holds
    /// an enabled component for it.
    pub enabled: bool,
    /// The effect's settings: a value of its effect component type.
    pub settings: Box<dyn PostProcessEffect>,
}

impl ProfileEntry {
    /// An enabled entry with `settings`.
    pub fn new(settings: impl PostProcessEffect) -> Self {
        Self {
            enabled: true,
            settings: Box::new(settings),
        }
    }

    /// A disabled entry with `settings`.
    pub fn disabled(settings: impl PostProcessEffect) -> Self {
        Self {
            enabled: false,
            ..Self::new(settings)
        }
    }

    /// The type UUID of the entry's effect component.
    pub fn type_uuid(&self) -> Uuid {
        self.settings.uuid()
    }

    /// The settings as `T`, when the entry is for the effect `T`.
    pub fn settings<T: PostProcessEffect>(&self) -> Option<&T> {
        self.settings.as_any().downcast_ref()
    }

    /// The entry as its file stores it.
    pub fn to_data(&self) -> ProfileEntryData {
        ProfileEntryData {
            type_uuid: self.type_uuid(),
            enabled: self.enabled,
            settings: self.settings.serialize().unwrap_or_default(),
        }
    }
}

/// A post-processing look saved as an asset (`.cxpostfx`), which any number
/// of global and local volumes reference through their `profile` field.
///
/// It holds at most one entry per effect, keyed by the effect component's
/// type UUID; the effect types are the components registered as
/// [`PostProcessEffect`]s. A volume uses an enabled entry unless its game
/// object holds an enabled component for the same effect, which replaces the
/// entry. Profiles do not blend with or layer over each other; local volumes
/// blend the settings each volume resolves to. See the module docs for the
/// file format.
#[derive(Default, TypeUuid)]
#[uuid = "2f2b4d1b-4d61-4751-b2e3-222579256f15"]
pub struct PostProcessProfile {
    entries: Vec<ProfileEntry>,
}

/// A [`PostProcessProfile`] as its file stores it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PostProcessProfileData {
    /// The entries, in the profile's order.
    pub entries: Vec<ProfileEntryData>,
}

/// A [`ProfileEntry`] as its file stores it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileEntryData {
    /// The effect component's type UUID.
    #[serde(rename = "type")]
    pub type_uuid: Uuid,
    /// Whether the entry is on; on when missing.
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    /// The effect component's serialized fields; each missing field takes
    /// its default.
    #[serde(default = "empty_settings")]
    pub settings: serde_json::Value,
}

fn enabled_by_default() -> bool {
    true
}

fn empty_settings() -> serde_json::Value {
    serde_json::Value::Object(Default::default())
}

impl PostProcessProfile {
    /// A profile of `entries`; an entry for a type an earlier one has
    /// replaces it.
    pub fn from_entries(entries: impl IntoIterator<Item = ProfileEntry>) -> Self {
        let mut profile = Self::default();
        for entry in entries {
            profile.insert(entry);
        }
        profile
    }

    /// The profile `data` describes, with each entry's settings deserialized
    /// through the component registry, as a scene's components are. An entry
    /// that cannot be used is skipped with a warning; see the module docs.
    pub fn from_data(
        data: PostProcessProfileData,
        types: &TypeRegistry,
        components: &ComponentRegistry,
    ) -> Self {
        let mut profile = Self::default();
        for entry in data.entries {
            if profile.has_entry(entry.type_uuid) {
                log::warn!(
                    "Skipping post-process profile entry {}: an earlier entry has the same type",
                    entry.type_uuid
                );
                continue;
            }
            match effect_from_json(types, components, entry.type_uuid, &entry.settings) {
                Ok(settings) => profile.entries.push(ProfileEntry {
                    enabled: entry.enabled,
                    settings,
                }),
                Err(error) => log::warn!(
                    "Skipping post-process profile entry {}: {error}",
                    entry.type_uuid
                ),
            }
        }
        profile
    }

    /// The profile as its file stores it.
    pub fn to_data(&self) -> PostProcessProfileData {
        PostProcessProfileData {
            entries: self.entries.iter().map(ProfileEntry::to_data).collect(),
        }
    }

    /// A copy of the profile, its settings copied through the registries.
    pub fn clone_with(&self, types: &TypeRegistry, components: &ComponentRegistry) -> Self {
        Self::from_data(self.to_data(), types, components)
    }

    /// The type UUIDs of the components registered as post-process effects,
    /// which a profile holds entries for, in UUID order.
    pub fn effect_types(types: &TypeRegistry) -> Vec<Uuid> {
        let mut effect_types = types.all_of(type_uuids!(ReflectPostProcessEffect));
        effect_types.sort();
        effect_types
    }

    /// The entries, in the profile's order.
    pub fn entries(&self) -> &[ProfileEntry] {
        &self.entries
    }

    /// The entries, in the profile's order, to edit.
    pub fn entries_mut(&mut self) -> &mut [ProfileEntry] {
        &mut self.entries
    }

    /// The entry for the effect component type `type_uuid`.
    pub fn entry(&self, type_uuid: Uuid) -> Option<&ProfileEntry> {
        self.entries
            .iter()
            .find(|entry| entry.type_uuid() == type_uuid)
    }

    /// The entry for the effect component type `type_uuid`, to edit.
    pub fn entry_mut(&mut self, type_uuid: Uuid) -> Option<&mut ProfileEntry> {
        self.entries
            .iter_mut()
            .find(|entry| entry.type_uuid() == type_uuid)
    }

    /// Whether the profile has an entry for the effect component type
    /// `type_uuid`.
    pub fn has_entry(&self, type_uuid: Uuid) -> bool {
        self.entry(type_uuid).is_some()
    }

    /// Puts `entry` in the profile, in place of the entry for its type if
    /// there is one, else after the others.
    pub fn insert(&mut self, entry: ProfileEntry) {
        match self.entry_mut(entry.type_uuid()) {
            Some(existing) => *existing = entry,
            None => self.entries.push(entry),
        }
    }

    /// Adds an enabled entry with the defaults of the effect component type
    /// `type_uuid`, unless the profile has one or the type is not a
    /// registered effect. Returns whether it added it.
    pub fn add_entry(&mut self, types: &TypeRegistry, type_uuid: Uuid) -> bool {
        if self.has_entry(type_uuid) {
            return false;
        }
        let settings = types
            .trait_meta::<ReflectPostProcessEffect>(type_uuid)
            .zip(types.trait_meta::<ReflectDefault>(type_uuid))
            .and_then(|(effect, default)| effect.get_boxed(default.default()).ok());
        let Some(settings) = settings else {
            return false;
        };
        self.entries.push(ProfileEntry {
            enabled: true,
            settings,
        });
        true
    }

    /// Removes the entry for the effect component type `type_uuid`. Returns
    /// whether the profile had one.
    pub fn remove_entry(&mut self, type_uuid: Uuid) -> bool {
        let count = self.entries.len();
        self.entries.retain(|entry| entry.type_uuid() != type_uuid);
        self.entries.len() != count
    }

    /// The effects the profile switches on: each enabled entry's settings,
    /// and `None` for a disabled or absent one.
    ///
    /// This maps the entries onto the fixed fields of [`PostProcessEffects`],
    /// one line per effect; #477 replaces those fields with a set keyed by
    /// type UUID and removes this mapping.
    pub fn effects(&self) -> PostProcessEffects {
        PostProcessEffects {
            tonemapping: self.effect::<ComponentTonemap>(),
            ambient_occlusion: self.effect::<ComponentAmbientOcclusion>(),
            fog: self.effect::<ComponentFog>(),
            depth_of_field: self.effect::<ComponentDepthOfField>(),
            bloom: self.effect::<ComponentBloom>(),
            color_grading: self.effect::<ComponentColorGrading>(),
            vignette: self.effect::<ComponentVignette>(),
            outline: self.effect::<ComponentOutline>(),
            anti_aliasing: self.effect::<ComponentAntiAliasing>(),
        }
    }

    /// The settings of `T`'s entry when it is enabled.
    fn effect<T: PostProcessEffect + TypeUuid + Copy>(&self) -> Option<T> {
        self.entry(T::type_uuid())
            .filter(|entry| entry.enabled)
            .and_then(ProfileEntry::settings::<T>)
            .copied()
    }

    /// A profile that keeps the look of the volume on `game_object`: its
    /// current profile's entries, if it has one, with each effect component
    /// on the object copied over them. An enabled component replaces its
    /// entry; a disabled one becomes a disabled entry only where the profile
    /// has none, since a disabled component leaves the profile's entry in
    /// effect.
    pub fn from_game_object(scene: &Scene, game_object: GameObject) -> Self {
        let registries = scene.registries();
        let types = registries.types.read();
        let components = registries.components.read();
        let mut profile = Self::of_volume(scene, game_object)
            .map(|profile| profile.read().clone_with(&types, &components))
            .unwrap_or_default();
        for type_uuid in Self::effect_types(&types) {
            let Some(prototype) = components.component(type_uuid) else {
                continue;
            };
            let Some(Some(json)) =
                scene.read_component_dyn(game_object, prototype, |component| component.serialize())
            else {
                continue;
            };
            let enabled = scene.is_component_enabled_dyn(game_object, prototype);
            if !enabled && profile.has_entry(type_uuid) {
                continue;
            }
            match effect_from_json(&types, &components, type_uuid, &json) {
                Ok(settings) => profile.insert(ProfileEntry { enabled, settings }),
                Err(error) => log::warn!("Skipping effect {type_uuid} of the volume: {error}"),
            }
        }
        profile
    }

    /// The loaded profile the volume on `game_object` references, global or
    /// local, the global volume's when the object has both, or `None` when
    /// it has none or the profile does not load. A profile that does not
    /// load is warned about once.
    pub fn of_volume(scene: &Scene, game_object: GameObject) -> Option<Ref<Self>> {
        let id = Self::volume_profile_id(scene, game_object)?;
        let loaded = scene
            .registries()
            .assets
            .read()
            .load_by_id::<PostProcessProfile>(id);
        match loaded {
            Ok(profile) => Some(profile),
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
}

/// The effect component of type `type_uuid` that `json` describes,
/// deserialized through its registered prototype as a scene component is.
fn effect_from_json(
    types: &TypeRegistry,
    components: &ComponentRegistry,
    type_uuid: Uuid,
    json: &serde_json::Value,
) -> Result<Box<dyn PostProcessEffect>, String> {
    let effect = types
        .trait_meta::<ReflectPostProcessEffect>(type_uuid)
        .ok_or("the type is unknown or not a post-process effect")?;
    let prototype = components
        .component(type_uuid)
        .ok_or("the type is not a registered component")?;
    let value = prototype
        .deserialize(json)
        .map_err(|error| format!("the settings do not deserialize: {error}"))?;
    effect
        .get_boxed(value)
        .map_err(|_| "the settings are not a post-process effect".to_owned())
}

/// Two profiles are equal when they hold entries for the same effects, each
/// with the same enabled flag and serialized settings, in any order.
impl PartialEq for PostProcessProfile {
    fn eq(&self, other: &Self) -> bool {
        self.entries.len() == other.entries.len()
            && self.entries.iter().all(|entry| {
                other
                    .entry(entry.type_uuid())
                    .is_some_and(|theirs| theirs.to_data() == entry.to_data())
            })
    }
}

impl fmt::Debug for PostProcessProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostProcessProfile")
            .field("entries", &self.to_data().entries)
            .finish()
    }
}

impl Serialize for PostProcessProfile {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_data().serialize(serializer)
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
        assets: &ReadOnlyAssetContext,
        path: &Path,
    ) -> Result<LoadedAsset<Self>, AssetError>
    where
        Self: Sized,
    {
        let error = |source: String| {
            AssetError::LoadError
                .with_path(path)
                .with_type(Self::asset_name())
                .with_source(source)
        };
        let file = std::fs::File::open(path).map_err(|err| error(err.to_string()))?;
        let data: PostProcessProfileData =
            serde_json::from_reader(BufReader::new(file)).map_err(|err| error(err.to_string()))?;
        let types = assets.registries.types.read();
        let components = assets.registries.components.read();
        Ok(LoadedAsset::new(Self::from_data(data, &types, &components)))
    }

    fn to_file(&self, path: &Path) -> Result<(), std::io::Error> {
        AssetRegistry::write_to_file(self, path)
    }
}

#[cfg(test)]
mod tests;
