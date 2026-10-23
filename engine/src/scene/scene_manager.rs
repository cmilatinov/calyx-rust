use crate::assets::AssetRegistry;
use crate::context::ReadOnlyRegistryContext;
use crate::core::{ReadOnlyRef, Ref};
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
    default_scene: Ref<Scene>,
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
                .expect("failed to load default scene");
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
            let mut simulation_scene = self.current_scene.restore_snapshot(snapshot);
            simulation_scene.start();
            self.simulation_scene = Some(simulation_scene);
        }

        self.simulation_running = true;
        log::info!("Scene simulation started");
    }

    /// Starts simulation from `scene` without replacing the authoring scene.
    ///
    /// If a simulation copy already exists, this resumes it rather than
    /// discarding paused state.
    pub fn start_simulation_from_scene(&mut self, scene: ReadOnlyRef<Scene>) {
        if self.simulation_scene.is_none() {
            let snapshot = scene.read().snapshot();
            let mut simulation_scene = self.current_scene.restore_snapshot(snapshot);
            simulation_scene.start();
            self.simulation_scene = Some(simulation_scene);
        }

        self.simulation_running = true;
        log::info!("Scene simulation started from configured scene");
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

    /// Takes apart every scene this manager reaches, the authoring scene, the
    /// default scene and each scene asset the registry has cached, leaving
    /// empty scenes in their place, and stops the simulation, dropping its
    /// scene.
    ///
    /// Component storages run the code of the build that added the component,
    /// so a scene holding a plugin's components must go before the plugin's
    /// assembly unloads. [`Self::resume_scenes`] rebuilds the scenes from the
    /// returned snapshots through whatever types are registered then.
    pub fn suspend_scenes(&mut self) -> SuspendedScenes {
        self.stop_simulation();
        let current = self.current_scene.snapshot();
        self.current_scene = Scene::new(self.current_scene.registries().clone());
        // The default scene stays here after the registry evicts it, such as
        // when its file is deleted.
        let mut assets = self.asset_registry.read().loaded_assets::<Scene>();
        if !assets
            .iter()
            .any(|asset| asset.ptr_id() == self.default_scene.ptr_id())
        {
            assets.push(self.default_scene.clone());
        }
        let assets = assets
            .into_iter()
            .map(|asset| {
                let snapshot = {
                    let mut scene = asset.write();
                    let snapshot = scene.snapshot();
                    *scene = Scene::new(scene.registries().clone());
                    snapshot
                };
                (asset, snapshot)
            })
            .collect();
        SuspendedScenes { current, assets }
    }

    /// Rebuilds the scenes [`Self::suspend_scenes`] took apart, each in the
    /// place it was taken from.
    pub fn resume_scenes(&mut self, suspended: SuspendedScenes) {
        self.current_scene = self.current_scene.restore_snapshot(suspended.current);
        for (asset, snapshot) in suspended.assets {
            let mut scene = asset.write();
            *scene = scene.restore_snapshot(snapshot);
        }
    }
}

/// Snapshots of the scenes [`SceneManager::suspend_scenes`] took apart.
#[must_use = "the suspended scenes stay empty until they are resumed"]
pub struct SuspendedScenes {
    current: SceneSnapshot,
    assets: Vec<(Ref<Scene>, SceneSnapshot)>,
}

#[cfg(test)]
mod tests {
    use crate::component::ComponentID;
    use crate::core::Ref;
    use crate::test_support::{test_game_context, test_scene};

    #[test]
    fn suspending_scenes_includes_a_default_scene_the_cache_evicted() {
        let mut game = test_game_context();
        // A default scene the asset registry no longer caches, as after its
        // file was deleted, which the manager still holds.
        let mut default_scene = test_scene();
        default_scene.create(
            Some(ComponentID {
                name: "Default".into(),
                ..Default::default()
            }),
            None,
        );
        let default_scene = Ref::new(default_scene);
        game.scenes.default_scene = default_scene.clone();
        let objects = |scene: &Ref<crate::scene::Scene>| scene.read().objects().count();
        assert_eq!(objects(&default_scene), 1);

        let suspended = game.scenes.suspend_scenes();
        assert_eq!(objects(&default_scene), 0);

        game.scenes.resume_scenes(suspended);
        assert_eq!(objects(&default_scene), 1);
    }
}
