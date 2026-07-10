use crate::assets::AssetRegistry;
use crate::context::ReadOnlyRegistryContext;
use crate::core::ReadOnlyRef;
use crate::input::Input;
use crate::resource::ResourceMap;
use crate::scene::{Scene, SceneSnapshot};
use std::path::PathBuf;

/// Metadata tracked alongside the current authoring scene.
#[derive(Default)]
pub struct SceneMeta {
    /// Source file for the loaded scene asset, when the scene originated from
    /// disk.
    pub file: Option<PathBuf>,
    /// Canonical asset metadata name for the loaded scene, when available.
    pub asset_name: Option<String>,
}

/// Owns the editable scene and the optional simulation copy used while the game
/// is running in-editor.
pub struct SceneManager {
    simulation_running: bool,
    current_scene: Scene,
    current_scene_meta: SceneMeta,
    simulation_scene: Option<Scene>,
    default_scene: ReadOnlyRef<Scene>,
    asset_registry: ReadOnlyRef<AssetRegistry>,
}

impl SceneManager {
    /// Creates a manager with an empty authoring scene and the project's
    /// default scene cached for resets.
    pub fn new(asset_registry_ref: ReadOnlyRef<AssetRegistry>) -> Self {
        log::info!("Initializing scene manager");
        let current_scene;
        let default_scene;
        {
            let asset_registry = asset_registry_ref.read();
            current_scene = asset_registry.new_empty_scene();
            default_scene = asset_registry
                .default_scene()
                .expect("failed to load default scene")
                .readonly();
        }
        Self {
            simulation_running: false,
            current_scene,
            current_scene_meta: Default::default(),
            simulation_scene: None,
            default_scene,
            asset_registry: asset_registry_ref,
        }
    }

    /// Replaces the current authoring scene with a fresh empty scene.
    pub fn load_empty_scene(&mut self) {
        self.stop_simulation();
        self.current_scene = self.asset_registry.read().new_empty_scene();
        self.current_scene_meta = Default::default();
        log::info!("Loaded empty scene");
    }

    /// Replaces the current authoring scene with a clone of the configured
    /// default scene asset.
    pub fn load_default_scene(&mut self) {
        self.stop_simulation();
        let snapshot = self.default_scene.read().snapshot();
        self.current_scene = self.current_scene.restore_snapshot(snapshot);
        self.current_scene_meta = Default::default();
        log::info!("Loaded default scene");
    }

    /// Loads `scene` into the authoring slot and records its asset path when
    /// available.
    pub fn load_scene(&mut self, scene: ReadOnlyRef<Scene>) {
        self.stop_simulation();

        let snapshot = scene.read().snapshot();
        self.current_scene = self.current_scene.restore_snapshot(snapshot);
        if let Some(asset_meta) = self.asset_registry.read().asset_meta_from_ref(&scene) {
            self.current_scene_meta = SceneMeta {
                file: asset_meta.path.clone(),
                asset_name: Some(asset_meta.name.clone()),
            };
            log::info!("Loaded scene asset {} ({})", asset_meta.name, asset_meta.id);
        } else {
            self.current_scene_meta = Default::default();
            log::info!("Loaded scene from in-memory asset reference");
        }
    }

    /// Updates the current scene source file when an editor workflow knows it.
    pub fn set_current_scene_file(&mut self, file: Option<PathBuf>) {
        self.current_scene_meta.file = file;
        self.current_scene_meta.asset_name = None;
    }

    /// Drops the simulation copy without modifying the authoring scene.
    pub fn unload_current_scene(&mut self) {
        self.simulation_scene = None;
        log::trace!("Unloaded simulation scene copy");
    }

    /// Starts simulation, cloning the current authoring scene on first run.
    pub fn start_simulation(&mut self) {
        if self.simulation_scene.is_none() {
            let snapshot = self.current_scene.snapshot();
            self.simulation_scene = Some(self.current_scene.restore_snapshot(snapshot));
        }

        self.simulation_running = true;
        log::info!("Scene simulation started");
    }

    /// Pauses simulation updates while preserving the simulation scene.
    pub fn pause_simulation(&mut self) {
        self.simulation_running = false;
        log::info!("Scene simulation paused");
    }

    /// Stops simulation and discards the simulation scene.
    pub fn stop_simulation(&mut self) {
        if self.simulation_running || self.simulation_scene.is_some() {
            log::info!("Scene simulation stopped");
        }
        self.simulation_scene = None;
        self.simulation_running = false;
    }

    /// Runs scene preparation on the active simulation target.
    pub fn prepare(&mut self) {
        self.simulation_scene_mut().prepare();
    }

    /// Advances the simulation scene when simulation is running.
    pub fn update(
        &mut self,
        registries: &ReadOnlyRegistryContext,
        resources: &mut ResourceMap,
        input: &Input,
    ) {
        if !self.simulation_running {
            return;
        }

        if let Some(scene) = &mut self.simulation_scene {
            scene.update(registries, resources, input);
        }
    }

    /// Returns `true` when a simulation copy currently exists.
    pub fn has_simulation_scene(&self) -> bool {
        self.simulation_scene.is_some()
    }

    /// Returns `true` when simulation updates are enabled.
    pub fn is_simulating(&self) -> bool {
        self.simulation_running
    }

    /// Returns the simulation scene when present, otherwise the authoring
    /// scene.
    pub fn simulation_scene(&self) -> &Scene {
        if let Some(scene) = &self.simulation_scene {
            return scene;
        }
        &self.current_scene
    }

    /// Returns the mutable scene targeted by editor actions during simulation.
    pub fn simulation_scene_mut(&mut self) -> &mut Scene {
        if let Some(scene) = &mut self.simulation_scene {
            return scene;
        }
        &mut self.current_scene
    }

    /// Returns metadata about the current authoring scene.
    pub fn current_scene_meta(&self) -> &SceneMeta {
        &self.current_scene_meta
    }

    /// Returns the current authoring scene.
    pub fn current_scene(&self) -> &Scene {
        &self.current_scene
    }

    /// Returns the current authoring scene mutably.
    pub fn current_scene_mut(&mut self) -> &mut Scene {
        &mut self.current_scene
    }

    /// Captures the current authoring scene for editor history.
    pub fn current_scene_snapshot(&self) -> SceneSnapshot {
        self.current_scene.snapshot()
    }

    /// Restores the current authoring scene from an editor history snapshot.
    pub fn restore_current_scene_snapshot(&mut self, snapshot: SceneSnapshot) {
        self.stop_simulation();
        self.current_scene = self.current_scene.restore_snapshot(snapshot);
    }
}
