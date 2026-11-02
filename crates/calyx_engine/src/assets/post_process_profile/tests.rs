//! Post-processing profiles: saving and loading, partial entries, and how a
//! volume's profile and the effect components on its object combine.

use super::*;
use crate::assets::{Asset, AssetRef};
use crate::component::{
    ComponentBloom, ComponentGlobalPostProcessVolume, ComponentLocalPostProcessVolume,
    ComponentTonemap, ComponentVignette,
};
use crate::context::ReadOnlyRegistryContext;
use crate::render::postprocess::{PostProcessEffects, PostProcessSettings};
use crate::scene::{GameObject, Scene};
use crate::test_support::test_registries_with_assets;
use notify::{Event, EventKind};
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

/// A temporary asset root, deleted when dropped.
struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("calyx-profile-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("the temp root is creatable");
        Self(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Registries over `root`, with the profile file at `path` registered.
fn registries_with_profile(root: &TempRoot, path: &Path) -> (ReadOnlyRegistryContext, Uuid) {
    let registries = test_registries_with_assets(vec![root.0.clone()]);
    let id = registries
        .assets
        .read()
        .register_file(path)
        .expect("the profile registers");
    (registries, id)
}

fn bloom(intensity: f32) -> ComponentBloom {
    ComponentBloom {
        intensity,
        ..Default::default()
    }
}

/// A profile with bloom at `intensity` and every other effect absent.
fn bloom_profile(intensity: f32) -> PostProcessProfile {
    PostProcessProfile {
        bloom: Some(ProfileEntry::new(bloom(intensity))),
        ..Default::default()
    }
}

/// A global volume referencing `profile`.
fn spawn_global(scene: &mut Scene, profile: Uuid) -> GameObject {
    let game_object = scene.create(None, None);
    scene.add_component(
        game_object,
        ComponentGlobalPostProcessVolume {
            profile: AssetRef::from_id(profile),
            ..Default::default()
        },
    );
    game_object
}

/// A local volume referencing `profile`.
fn spawn_local(scene: &mut Scene, profile: Uuid) -> GameObject {
    let game_object = scene.create(None, None);
    scene.add_component(
        game_object,
        ComponentLocalPostProcessVolume {
            profile: AssetRef::from_id(profile),
            ..Default::default()
        },
    );
    game_object
}

/// Writes `profile` to `path` the way the editor saves it.
fn save(profile: &PostProcessProfile, path: &Path) {
    profile.to_file(path).expect("the profile saves");
}

/// Rewrites `path` with `profile` and runs the hot reload the file watcher
/// would trigger.
fn rewrite_and_hot_reload(registries: &ReadOnlyRegistryContext, path: &Path, profile: &str) {
    std::fs::write(path, profile).expect("the profile is writable");
    let registry = registries.assets.read();
    registry.recv_notify_event(
        Event::new(EventKind::Modify(notify::event::ModifyKind::Any)).add_path(path.to_path_buf()),
    );
    std::thread::sleep(Duration::from_millis(200));
    registry.reload_assets();
}

#[test]
fn a_profile_saves_and_loads_through_the_asset_registry() {
    let root = TempRoot::new();
    let path = root.path("look.cxpostfx");
    let profile = PostProcessProfile {
        tonemapping: Some(ProfileEntry::new(ComponentTonemap {
            exposure_ev: 1.5,
            ..Default::default()
        })),
        bloom: Some(ProfileEntry {
            enabled: false,
            settings: bloom(0.8),
        }),
        vignette: Some(ProfileEntry::new(ComponentVignette {
            intensity: 0.7,
            ..Default::default()
        })),
        ..Default::default()
    };
    save(&profile, &path);

    let (registries, id) = registries_with_profile(&root, &path);
    let loaded = registries
        .assets
        .read()
        .load_by_id::<PostProcessProfile>(id)
        .expect("the profile loads");

    assert_eq!(*loaded.read(), profile);
}

#[test]
fn a_partial_entry_loads_the_effects_defaults() {
    let root = TempRoot::new();
    let path = root.path("partial.cxpostfx");
    std::fs::write(
        &path,
        r#"{ "bloom": { "settings": { "intensity": 2.0 } }, "vignette": { "enabled": false } }"#,
    )
    .unwrap();

    let (registries, id) = registries_with_profile(&root, &path);
    let loaded = *registries
        .assets
        .read()
        .load_by_id::<PostProcessProfile>(id)
        .expect("the profile loads")
        .read();

    assert_eq!(
        loaded,
        PostProcessProfile {
            bloom: Some(ProfileEntry::new(ComponentBloom {
                intensity: 2.0,
                threshold: 1.0,
                knee: 0.5,
                radius: 6,
            })),
            vignette: Some(ProfileEntry {
                enabled: false,
                settings: ComponentVignette::default(),
            }),
            ..Default::default()
        }
    );
}

#[test]
fn an_enabled_effect_component_replaces_the_profile_entry_and_a_disabled_or_absent_one_keeps_it() {
    let root = TempRoot::new();
    let path = root.path("look.cxpostfx");
    save(&bloom_profile(0.3), &path);
    let (registries, id) = registries_with_profile(&root, &path);
    let mut scene = registries.scene();

    let absent = spawn_global(&mut scene, id);
    let enabled = spawn_global(&mut scene, id);
    scene.add_component(enabled, bloom(0.9));
    let disabled = spawn_global(&mut scene, id);
    scene.add_component(disabled, bloom(0.9));
    scene.set_component_enabled::<ComponentBloom>(disabled, false);

    let bloom_of = |object| PostProcessSettings::from_game_object(&scene, object).bloom;
    assert_eq!(bloom_of(absent), Some(bloom(0.3)), "absent component");
    assert_eq!(bloom_of(enabled), Some(bloom(0.9)), "enabled component");
    assert_eq!(bloom_of(disabled), Some(bloom(0.3)), "disabled component");
}

#[test]
fn a_disabled_profile_entry_is_off_unless_an_enabled_component_replaces_it() {
    let root = TempRoot::new();
    let path = root.path("look.cxpostfx");
    let profile = PostProcessProfile {
        vignette: Some(ProfileEntry {
            enabled: false,
            settings: ComponentVignette::default(),
        }),
        ..Default::default()
    };
    save(&profile, &path);
    let (registries, id) = registries_with_profile(&root, &path);
    let mut scene = registries.scene();

    let plain = spawn_global(&mut scene, id);
    let overridden = spawn_global(&mut scene, id);
    let vignette = ComponentVignette {
        intensity: 0.9,
        ..Default::default()
    };
    scene.add_component(overridden, vignette);

    assert_eq!(
        PostProcessSettings::from_game_object(&scene, plain).vignette,
        None
    );
    assert_eq!(
        PostProcessSettings::from_game_object(&scene, overridden).vignette,
        Some(vignette)
    );
}

#[test]
fn a_profile_without_tone_mapping_leaves_the_tone_mapping_defaults() {
    let root = TempRoot::new();
    let path = root.path("look.cxpostfx");
    save(&bloom_profile(0.3), &path);
    let (registries, id) = registries_with_profile(&root, &path);
    let mut scene = registries.scene();
    let volume = spawn_global(&mut scene, id);

    let settings = PostProcessSettings::from_game_object(&scene, volume);

    assert_eq!(settings.tonemapping, ComponentTonemap::default());
    assert_eq!(settings.bloom, Some(bloom(0.3)));
}

#[test]
fn a_volume_whose_profile_does_not_load_uses_its_components_alone() {
    let registries = crate::test_support::test_registries();
    let mut scene = registries.scene();
    let volume = spawn_global(&mut scene, Uuid::new_v4());
    scene.add_component(volume, bloom(0.9));

    assert_eq!(
        PostProcessEffects::from_game_object(&scene, volume),
        PostProcessEffects {
            bloom: Some(bloom(0.9)),
            ..Default::default()
        }
    );
}

#[test]
fn two_volumes_sharing_a_profile_both_follow_an_edit_to_it() {
    let root = TempRoot::new();
    let path = root.path("look.cxpostfx");
    save(&bloom_profile(0.3), &path);
    let (registries, id) = registries_with_profile(&root, &path);
    let mut scene = registries.scene();
    let global = spawn_global(&mut scene, id);
    let local = spawn_local(&mut scene, id);
    let effects_of = |scene: &Scene, object| PostProcessEffects::from_game_object(scene, object);
    assert_eq!(effects_of(&scene, global).bloom, Some(bloom(0.3)));
    assert_eq!(effects_of(&scene, local).bloom, Some(bloom(0.3)));

    let profile = registries
        .assets
        .read()
        .load_by_id::<PostProcessProfile>(id)
        .unwrap();
    profile.write().bloom = Some(ProfileEntry::new(bloom(0.6)));

    assert_eq!(effects_of(&scene, global).bloom, Some(bloom(0.6)));
    assert_eq!(effects_of(&scene, local).bloom, Some(bloom(0.6)));
}

#[test]
fn a_hot_reload_of_the_profile_changes_the_resolved_settings() {
    let root = TempRoot::new();
    let path = root.path("look.cxpostfx");
    save(&bloom_profile(0.3), &path);
    let (registries, id) = registries_with_profile(&root, &path);
    let mut scene = registries.scene();
    spawn_global(&mut scene, id);
    let resolve = |scene: &Scene| PostProcessSettings::resolve(scene, &Default::default());
    assert_eq!(resolve(&scene).bloom, Some(bloom(0.3)));

    rewrite_and_hot_reload(
        &registries,
        &path,
        r#"{ "bloom": { "settings": { "intensity": 1.25 } } }"#,
    );

    assert_eq!(resolve(&scene).bloom, Some(bloom(1.25)));
}

#[test]
fn volumes_saved_without_a_profile_still_load() {
    let global: ComponentGlobalPostProcessVolume =
        serde_json::from_str(r#"{ "priority": 3 }"#).unwrap();
    let local: ComponentLocalPostProcessVolume =
        serde_json::from_str(r#"{ "weight": 0.5 }"#).unwrap();

    assert_eq!(global.priority, 3);
    assert!(global.profile.id().is_nil());
    assert_eq!(local.weight, 0.5);
    assert!(local.profile.id().is_nil());
}

#[test]
fn a_profile_from_a_volume_copies_its_effect_components_and_keeps_the_look() {
    let registries = crate::test_support::test_registries();
    let mut scene = registries.scene();
    let volume = scene.create(None, None);
    scene.add_component(volume, ComponentGlobalPostProcessVolume::default());
    scene.add_component(volume, bloom(0.9));
    let vignette = ComponentVignette {
        intensity: 0.7,
        ..Default::default()
    };
    scene.add_component(volume, vignette);
    scene.set_component_enabled::<ComponentVignette>(volume, false);
    let before = PostProcessSettings::from_game_object(&scene, volume);

    let profile = PostProcessProfile::from_game_object(&scene, volume);

    assert_eq!(
        profile,
        PostProcessProfile {
            bloom: Some(ProfileEntry::new(bloom(0.9))),
            vignette: Some(ProfileEntry {
                enabled: false,
                settings: vignette,
            }),
            ..Default::default()
        }
    );
    assert_eq!(PostProcessSettings::from(profile.effects()), before);
}
