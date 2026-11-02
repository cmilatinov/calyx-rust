//! Editing post-processing profile assets, and creating them from the
//! content browser and from a volume's inspector.

use std::path::{Path, PathBuf};

use calyx_engine::assets::post_process_profile::PostProcessProfile;
use calyx_engine::assets::{Asset, AssetRef};
use calyx_engine::component::{ComponentGlobalPostProcessVolume, ComponentLocalPostProcessVolume};
use calyx_engine::context::GameContext;
use calyx_engine::reflect::{Reflect, ReflectDefault, TypeInfo};
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::utils::TypeUuid;
use egui::Ui;
use uuid::Uuid;

use crate::inspector::asset_inspector::{AssetInspector, ReflectAssetInspector};
use crate::inspector::component_inspector::{type_display_name, ComponentInspector};
use crate::inspector::inspector_registry::InspectorRegistry;
use crate::inspector::type_inspector::InspectorContext;
use crate::scene_edit::SceneEdit;

/// The file name a new profile takes, before a number makes it unique.
const NEW_PROFILE_STEM: &str = "post_process_profile";

/// Edits a [`PostProcessProfile`]: each entry draws as an effect component
/// does, with its enabled checkbox and a Remove entry in its header's menu,
/// and "Add Effect" adds the effects the profile lacks. Edits apply to the
/// loaded profile, so every volume that uses it renders them on the next
/// frame; Save in the asset's menu writes the file.
#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, AssetInspector)]
pub struct PostProcessProfileInspector;

impl AssetInspector for PostProcessProfileInspector {
    fn target_type_uuid(&self) -> Uuid {
        PostProcessProfile::type_uuid()
    }

    fn show_inspector(
        &self,
        ui: &mut Ui,
        game: &mut GameContext,
        inspectors: &InspectorRegistry,
        asset_id: Uuid,
    ) {
        let Ok(profile) = game
            .assets
            .registries
            .assets
            .read()
            .load_by_id::<PostProcessProfile>(asset_id)
        else {
            return;
        };
        let assets = game.assets.lock_read();
        let types = assets.registries.types.read();
        let scene = game.scenes.simulation_scene();
        let view = scene.view();
        let component_inspector = ComponentInspector::new(inspectors);
        let mut profile = profile.write();
        let mut removals = Vec::new();
        for type_uuid in PostProcessProfile::effect_types() {
            let Some(entry) = profile.entry_mut(type_uuid) else {
                continue;
            };
            let Some(TypeInfo::Struct(type_info)) = types.type_info_by_id(type_uuid) else {
                continue;
            };
            let ctx = InspectorContext {
                assets: &assets.registries,
                scene: &view,
                game_object: scene.root(),
                parent: None,
                type_info,
                field_name: None,
            };
            let response = ui
                .push_id(type_uuid, |ui| {
                    component_inspector.show_unpinned(
                        ui,
                        &ctx,
                        entry.settings,
                        Some(*entry.enabled),
                    )
                })
                .inner;
            if let Some(enabled) = response.enabled {
                *entry.enabled = enabled;
            }
            if response.remove {
                removals.push(type_uuid);
            }
        }
        for type_uuid in removals {
            profile.remove_entry(type_uuid);
        }
        let missing: Vec<_> = PostProcessProfile::effect_types()
            .into_iter()
            .filter(|type_uuid| !profile.has_entry(*type_uuid))
            .filter_map(|type_uuid| Some((type_uuid, type_display_name(&types, type_uuid)?)))
            .collect();
        ui.add_enabled_ui(!missing.is_empty(), |ui| {
            ui.menu_button("Add Effect", |ui| {
                for (type_uuid, name) in missing {
                    if ui.button(name).clicked() {
                        profile.add_entry(type_uuid);
                        ui.close_menu();
                    }
                }
            });
        });
    }
}

impl PostProcessProfileInspector {
    /// Whether `game_object` holds a global or local post-process volume,
    /// which a profile can be created from.
    pub fn is_volume(scene: &Scene, game_object: GameObject) -> bool {
        scene
            .read_component::<ComponentGlobalPostProcessVolume, _, _>(game_object, |_| ())
            .is_some()
            || scene
                .read_component::<ComponentLocalPostProcessVolume, _, _>(game_object, |_| ())
                .is_some()
    }

    /// Writes an empty profile at a free name in `folder`, which the asset
    /// registry picks up as a new asset, and returns its path.
    pub fn create_in_folder(folder: &Path) -> std::io::Result<PathBuf> {
        let path = free_profile_path(folder);
        PostProcessProfile::default().to_file(&path)?;
        Ok(path)
    }

    /// Creates a profile at `path` from the volume on `game_object` through
    /// `edit`, and returns its asset id. The profile takes the volume's
    /// look, its current profile's entries with the object's effect
    /// components copied over them (see
    /// [`PostProcessProfile::from_game_object`]); the volume then references
    /// the new profile and the effect components leave the object, so the
    /// look is unchanged and editing the profile changes it. `path` must be a
    /// `.cxpostfx` file in an asset root, checked before anything is written;
    /// an existing profile there is replaced.
    pub fn create_from_volume(
        edit: &mut SceneEdit,
        game: &mut GameContext,
        game_object: GameObject,
        path: &Path,
    ) -> Result<Uuid, String> {
        let scene = game.scenes.simulation_scene();
        if !Self::is_volume(scene, game_object) {
            return Err(format!(
                "{} has no post-process volume to create a profile from",
                scene.name(game_object)
            ));
        }
        // Checked before writing, so a refused destination is never
        // overwritten: a file of another type, or one the registry would not
        // take as an asset.
        if !path.extension().is_some_and(|extension| {
            PostProcessProfile::file_extensions().contains(&extension.to_str().unwrap_or_default())
        }) {
            return Err(format!("{} is not a .cxpostfx file", path.display()));
        }
        if !game.assets.registries.assets.read().is_in_asset_root(path) {
            return Err(format!("{} is outside every asset root", path.display()));
        }
        let profile = PostProcessProfile::from_game_object(scene, game_object);
        profile
            .to_file(path)
            .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
        let registry_ref = game.assets.registries.assets.clone();
        let registry = registry_ref.read();
        let id = dunce::canonicalize(path)
            .map_err(|error| error.to_string())
            .and_then(|path| registry.register_file(&path).map_err(|e| e.to_string()))
            .map_err(|error| format!("{} is not usable as an asset: {error}", path.display()))?;
        // A profile file written over an existing one keeps its id, and the
        // registry may hold the old value; give it the new one.
        if let Some(loaded) = registry.loaded_by_id::<PostProcessProfile>(id) {
            *loaded.write() = profile;
        }
        drop(registry);

        let scene = edit.scene_mut(game);
        let reference = || AssetRef::<PostProcessProfile>::from_id(id);
        scene.write_component(
            game_object,
            |volume: &mut ComponentGlobalPostProcessVolume| volume.profile = reference(),
        );
        scene.write_component(
            game_object,
            |volume: &mut ComponentLocalPostProcessVolume| volume.profile = reference(),
        );
        for type_uuid in PostProcessProfile::effect_types() {
            scene.remove_component_dyn(game_object, type_uuid);
        }
        log::info!(
            "Created post-process profile {} ({id}) from {}",
            path.display(),
            scene.name(game_object)
        );
        Ok(id)
    }

    /// Asks for a file in the open scene's folder, then creates a profile
    /// there from the volume on `game_object` as one undo step.
    pub fn create_from_volume_with_dialog(
        game: &mut GameContext,
        game_object: GameObject,
    ) -> Option<SceneEdit> {
        let folder = game
            .scenes
            .current_scene_meta()
            .file
            .as_ref()
            .and_then(|file| file.parent().map(Path::to_path_buf));
        let mut dialog = rfd::FileDialog::new()
            .set_file_name(format!("{NEW_PROFILE_STEM}.cxpostfx"))
            .add_filter("cxpostfx", &["cxpostfx"]);
        if let Some(folder) = folder {
            dialog = dialog.set_directory(folder);
        }
        let path = dialog.save_file()?;
        let mut edit = SceneEdit::begin("Create post-process profile");
        match Self::create_from_volume(&mut edit, game, game_object, &path) {
            Ok(_) => Some(edit),
            Err(error) => {
                log::error!("Failed to create a post-process profile: {error}");
                edit.cancel();
                None
            }
        }
    }
}

/// `folder`'s first `post_process_profile[_N].cxpostfx` that does not exist.
fn free_profile_path(folder: &Path) -> PathBuf {
    let name = |n: usize| match n {
        0 => format!("{NEW_PROFILE_STEM}.cxpostfx"),
        n => format!("{NEW_PROFILE_STEM}_{n}.cxpostfx"),
    };
    (0..)
        .map(|n| folder.join(name(n)))
        .find(|path| !path.exists())
        .expect("some numbered name is free")
}

#[cfg(test)]
mod tests;
