//! Post-processing profiles: the file format, partial and unknown entries,
//! and how a volume's profile and the effect components on its object
//! combine.

use super::*;
use crate::assets::{Asset, AssetRef};
use crate::component::{
    ComponentBloom, ComponentCamera, ComponentFog, ComponentGlobalPostProcessVolume,
    ComponentLocalPostProcessVolume, ComponentOutline, ComponentTonemap, ComponentVignette,
    FogMode, TonemappingPreset,
};
use crate::context::ReadOnlyRegistryContext;
use crate::render::postprocess::PostProcessSettings;
use crate::scene::{GameObject, Scene};
use crate::test_support::{test_registries, test_registries_with_assets};
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

/// The profile at `id`, loaded through `registries`.
fn load(registries: &ReadOnlyRegistryContext, id: Uuid) -> crate::core::Ref<PostProcessProfile> {
    registries
        .assets
        .read()
        .load_by_id::<PostProcessProfile>(id)
        .expect("the profile loads")
}

fn bloom(intensity: f32) -> ComponentBloom {
    ComponentBloom {
        intensity,
        ..Default::default()
    }
}

/// A profile with bloom at `intensity` and every other effect absent.
fn bloom_profile(intensity: f32) -> PostProcessProfile {
    PostProcessProfile::from_entries([ProfileEntry::new(bloom(intensity))])
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
fn a_profile_saves_as_a_list_of_entries_keyed_by_type_uuid() {
    let profile = PostProcessProfile::from_entries([
        ProfileEntry::new(ComponentTonemap {
            exposure_ev: 1.5,
            ..Default::default()
        }),
        ProfileEntry::disabled(bloom(0.75)),
    ]);

    let saved = serde_json::to_value(profile.to_data()).unwrap();

    assert_eq!(
        saved,
        serde_json::json!({
            "entries": [
                {
                    "type": ComponentTonemap::type_uuid(),
                    "enabled": true,
                    "settings": { "preset": "Aces", "exposure_ev": 1.5 },
                },
                {
                    "type": ComponentBloom::type_uuid(),
                    "enabled": false,
                    "settings": { "threshold": 1.0, "knee": 0.5, "intensity": 0.75, "radius": 6 },
                },
            ]
        })
    );
}

#[test]
fn enabled_effect_access_borrows_every_registered_type_and_skips_disabled_entries() {
    let registries = test_registries();
    let types = registries.types.read();
    let mut profile = PostProcessProfile::default();
    let effect_types = PostProcessProfile::effect_types(&types);
    assert!(!effect_types.is_empty(), "registered effects are exercised");
    for type_uuid in &effect_types {
        assert!(profile.add_entry(&types, *type_uuid));
    }
    for effect in profile.enabled_effects() {
        let entry = profile.entry(effect.uuid()).unwrap();
        assert!(
            std::ptr::eq(effect, entry.settings.as_ref()),
            "settings are borrowed, not cloned"
        );
    }
    assert_eq!(profile.enabled_effects().count(), effect_types.len());
    let bloom = profile.effect::<ComponentBloom>().unwrap();
    assert!(std::ptr::eq(
        bloom,
        profile
            .entry(ComponentBloom::type_uuid())
            .unwrap()
            .settings()
            .unwrap()
    ));
    profile
        .entry_mut(ComponentBloom::type_uuid())
        .unwrap()
        .enabled = false;
    assert!(profile.effect::<ComponentBloom>().is_none());
    assert_eq!(profile.enabled_effects().count(), effect_types.len() - 1);
}

#[test]
fn a_profile_saves_and_loads_through_the_asset_registry() {
    let root = TempRoot::new();
    let path = root.path("look.cxpostfx");
    let profile = PostProcessProfile::from_entries([
        ProfileEntry::new(ComponentTonemap {
            exposure_ev: 1.5,
            ..Default::default()
        }),
        ProfileEntry::disabled(bloom(0.8)),
        ProfileEntry::new(ComponentVignette {
            intensity: 0.7,
            ..Default::default()
        }),
    ]);
    save(&profile, &path);

    let (registries, id) = registries_with_profile(&root, &path);

    assert_eq!(*load(&registries, id).read(), profile);
}

#[test]
fn a_fog_entry_round_trips_and_switches_fog_on() {
    let root = TempRoot::new();
    let path = root.path("fog.cxpostfx");
    let fog = ComponentFog {
        mode: FogMode::Exponential,
        color: egui::Color32::from_rgb(120, 160, 200),
        intensity: 1.5,
        start: 10.0,
        end: 400.0,
        density: 0.004,
        opacity: 0.9,
    };
    let profile = PostProcessProfile::from_entries([ProfileEntry::new(fog)]);
    save(&profile, &path);
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        saved["entries"][0]["type"],
        serde_json::json!(ComponentFog::type_uuid()),
        "the entry is keyed by the fog component's type"
    );
    assert_eq!(saved["entries"][0]["settings"]["mode"], "Exponential");

    let (registries, id) = registries_with_profile(&root, &path);
    let loaded = load(&registries, id);

    assert_eq!(*loaded.read(), profile);
    assert_eq!(loaded.read().effect::<ComponentFog>(), Some(&fog));
    let mut scene = registries.scene();
    spawn_global(&mut scene, id);
    assert_eq!(
        PostProcessSettings::resolve(&scene, &nalgebra_glm::Vec3::zeros())
            .get::<ComponentFog>()
            .copied(),
        Some(fog),
        "a volume using the profile renders with its fog"
    );
}

#[test]
fn an_outline_entry_round_trips_and_switches_outlines_on() {
    let root = TempRoot::new();
    let path = root.path("outline.cxpostfx");
    let outline = crate::component::ComponentOutline {
        styles: vec![
            crate::component::OutlineStyle {
                color: egui::Color32::from_rgb(0x12, 0x12, 0x12),
                width: 3.0,
                inner_width: 1.0,
                inner_opacity: 0.55,
                opacity: 0.9,
            },
            crate::component::OutlineStyle {
                color: egui::Color32::from_rgb(40, 20, 10),
                width: 2.0,
                inner_width: 2.0,
                inner_opacity: 0.25,
                opacity: 1.0,
            },
        ],
        depth_threshold: 0.02,
        silhouette_threshold: 0.05,
        normal_threshold: 35.0,
    };
    let profile = PostProcessProfile::from_entries([ProfileEntry::new(outline.clone())]);
    save(&profile, &path);
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        saved["entries"][0]["type"],
        serde_json::json!(crate::component::ComponentOutline::type_uuid()),
        "the entry is keyed by the outline component's type"
    );
    assert_eq!(
        saved["entries"][0]["settings"]["styles"][1]["inner_opacity"], 0.25,
        "the file holds the style list"
    );

    let (registries, id) = registries_with_profile(&root, &path);
    let loaded = load(&registries, id);

    assert_eq!(*loaded.read(), profile);
    assert_eq!(loaded.read().effect::<ComponentOutline>(), Some(&outline));
    let mut scene = registries.scene();
    spawn_global(&mut scene, id);
    assert_eq!(
        PostProcessSettings::resolve(&scene, &nalgebra_glm::Vec3::zeros())
            .get::<ComponentOutline>()
            .cloned(),
        Some(outline),
        "a volume using the profile renders with its outlines"
    );
}

#[test]
fn a_neutral_tone_mapping_entry_round_trips_and_selects_the_curve() {
    let root = TempRoot::new();
    let path = root.path("neutral.cxpostfx");
    let tonemap = ComponentTonemap {
        preset: TonemappingPreset::Neutral,
        exposure_ev: 0.5,
    };
    let profile = PostProcessProfile::from_entries([ProfileEntry::new(tonemap)]);
    save(&profile, &path);
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved["entries"][0]["settings"]["preset"], "Neutral");

    let (registries, id) = registries_with_profile(&root, &path);
    let loaded = load(&registries, id);

    assert_eq!(*loaded.read(), profile);
    let mut scene = registries.scene();
    spawn_global(&mut scene, id);
    assert_eq!(
        PostProcessSettings::resolve(&scene, &nalgebra_glm::Vec3::zeros())
            .get::<ComponentTonemap>()
            .copied()
            .unwrap_or_default(),
        tonemap,
        "a volume using the profile tone-maps with the neutral curve"
    );
}

#[test]
fn an_empty_profile_round_trips() {
    let root = TempRoot::new();
    let path = root.path("empty.cxpostfx");
    save(&PostProcessProfile::default(), &path);

    let (registries, id) = registries_with_profile(&root, &path);

    assert_eq!(*load(&registries, id).read(), PostProcessProfile::default());
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&path).unwrap())
            .unwrap(),
        serde_json::json!({ "entries": [] })
    );
}

#[test]
fn a_partial_entry_loads_the_effects_defaults() {
    let root = TempRoot::new();
    let path = root.path("partial.cxpostfx");
    let file = serde_json::json!({
        "entries": [
            { "type": ComponentBloom::type_uuid(), "settings": { "intensity": 2.0 } },
            { "type": ComponentVignette::type_uuid(), "enabled": false },
        ]
    });
    std::fs::write(&path, file.to_string()).unwrap();

    let (registries, id) = registries_with_profile(&root, &path);

    assert_eq!(
        *load(&registries, id).read(),
        PostProcessProfile::from_entries([
            ProfileEntry::new(ComponentBloom {
                intensity: 2.0,
                threshold: 1.0,
                knee: 0.5,
                radius: 6,
            }),
            ProfileEntry::disabled(ComponentVignette::default()),
        ])
    );
}

#[test]
fn an_unknown_or_non_effect_type_is_skipped_and_the_rest_loads() {
    let root = TempRoot::new();
    let path = root.path("unknown.cxpostfx");
    let file = serde_json::json!({
        "entries": [
            { "type": Uuid::new_v4(), "settings": { "strength": 1.0 } },
            { "type": ComponentCamera::type_uuid(), "settings": {} },
            { "type": ComponentBloom::type_uuid(), "settings": { "intensity": 2.0 } },
            { "type": ComponentBloom::type_uuid(), "settings": { "intensity": 3.0 } },
        ]
    });
    std::fs::write(&path, file.to_string()).unwrap();

    let (registries, id) = registries_with_profile(&root, &path);

    assert_eq!(*load(&registries, id).read(), bloom_profile(2.0));
}

#[test]
fn the_effect_types_are_the_registered_effect_components() {
    let registries = test_registries();
    let types = registries.types.read();

    let mut effect_types = PostProcessProfile::effect_types(&types);
    effect_types.sort();
    let mut expected = vec![
        ComponentTonemap::type_uuid(),
        crate::component::ComponentAmbientOcclusion::type_uuid(),
        crate::component::ComponentDepthOfField::type_uuid(),
        ComponentBloom::type_uuid(),
        crate::component::ComponentColorGrading::type_uuid(),
        ComponentVignette::type_uuid(),
        crate::component::ComponentAntiAliasing::type_uuid(),
        ComponentFog::type_uuid(),
        crate::component::ComponentOutline::type_uuid(),
    ];
    expected.sort();

    assert_eq!(effect_types, expected);
}

#[test]
fn entries_are_added_with_their_defaults_and_removed_by_type() {
    let registries = test_registries();
    let types = registries.types.read();
    let mut profile = PostProcessProfile::default();

    assert!(profile.add_entry(&types, ComponentVignette::type_uuid()));
    assert!(!profile.add_entry(&types, ComponentVignette::type_uuid()));
    assert!(!profile.add_entry(&types, ComponentCamera::type_uuid()));
    assert!(profile.has_entry(ComponentVignette::type_uuid()));
    assert_eq!(
        profile,
        PostProcessProfile::from_entries([ProfileEntry::new(ComponentVignette::default())])
    );

    assert!(profile.remove_entry(ComponentVignette::type_uuid()));
    assert!(!profile.remove_entry(ComponentVignette::type_uuid()));
    assert_eq!(profile, PostProcessProfile::default());
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

    let bloom_of = |object| {
        PostProcessSettings::from_game_object(&scene, object)
            .get::<ComponentBloom>()
            .copied()
    };
    assert_eq!(bloom_of(absent), Some(bloom(0.3)), "absent component");
    assert_eq!(bloom_of(enabled), Some(bloom(0.9)), "enabled component");
    assert_eq!(bloom_of(disabled), Some(bloom(0.3)), "disabled component");
}

#[test]
fn a_disabled_profile_entry_is_off_unless_an_enabled_component_replaces_it() {
    let root = TempRoot::new();
    let path = root.path("look.cxpostfx");
    let profile =
        PostProcessProfile::from_entries([ProfileEntry::disabled(ComponentVignette::default())]);
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
        PostProcessSettings::from_game_object(&scene, plain)
            .get::<ComponentVignette>()
            .copied(),
        None
    );
    assert_eq!(
        PostProcessSettings::from_game_object(&scene, overridden)
            .get::<ComponentVignette>()
            .copied(),
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

    assert_eq!(
        settings
            .get::<ComponentTonemap>()
            .copied()
            .unwrap_or_default(),
        ComponentTonemap::default()
    );
    assert_eq!(settings.get::<ComponentBloom>().copied(), Some(bloom(0.3)));
}

#[test]
fn a_volume_whose_profile_does_not_load_uses_its_components_alone() {
    let registries = test_registries();
    let mut scene = registries.scene();
    let volume = spawn_global(&mut scene, Uuid::new_v4());
    scene.add_component(volume, bloom(0.9));

    assert_eq!(
        PostProcessSettings::from_game_object(&scene, volume),
        PostProcessSettings::default().with(bloom(0.9))
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
    let effects_of = |scene: &Scene, object| PostProcessSettings::from_game_object(scene, object);
    assert_eq!(
        effects_of(&scene, global).get::<ComponentBloom>().copied(),
        Some(bloom(0.3))
    );
    assert_eq!(
        effects_of(&scene, local).get::<ComponentBloom>().copied(),
        Some(bloom(0.3))
    );

    load(&registries, id)
        .write()
        .insert(ProfileEntry::new(bloom(0.6)));

    assert_eq!(
        effects_of(&scene, global).get::<ComponentBloom>().copied(),
        Some(bloom(0.6))
    );
    assert_eq!(
        effects_of(&scene, local).get::<ComponentBloom>().copied(),
        Some(bloom(0.6))
    );
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
    assert_eq!(
        resolve(&scene).get::<ComponentBloom>().copied(),
        Some(bloom(0.3))
    );

    let file = serde_json::json!({
        "entries": [{ "type": ComponentBloom::type_uuid(), "settings": { "intensity": 1.25 } }]
    });
    rewrite_and_hot_reload(&registries, &path, &file.to_string());

    assert_eq!(
        resolve(&scene).get::<ComponentBloom>().copied(),
        Some(bloom(1.25))
    );
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
    let registries = test_registries();
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
        PostProcessProfile::from_entries([
            ProfileEntry::new(bloom(0.9)),
            ProfileEntry::disabled(vignette),
        ])
    );
    assert_eq!(PostProcessSettings::from_profile(&profile), before);
}
