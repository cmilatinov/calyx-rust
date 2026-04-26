use crate::assets::AssetRegistry;
use crate::context::ReadOnlyRegistryContext;
use crate::core::ReadOnlyRef;
use crate::input::Input;
use crate::resource::ResourceMap;
use crate::scene::Scene;
use std::path::PathBuf;

#[derive(Default)]
pub struct SceneMeta {
    pub file: Option<PathBuf>,
}

pub struct SceneManager {
    simulation_running: bool,
    current_scene: Scene,
    current_scene_meta: SceneMeta,
    simulation_scene: Option<Scene>,
    default_scene: ReadOnlyRef<Scene>,
    asset_registry: ReadOnlyRef<AssetRegistry>,
}

impl SceneManager {
    pub fn new(asset_registry_ref: ReadOnlyRef<AssetRegistry>) -> Self {
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

    pub fn load_empty_scene(&mut self) {
        self.stop_simulation();
        self.current_scene = self.asset_registry.read().new_empty_scene();
    }

    pub fn load_default_scene(&mut self) {
        self.stop_simulation();
        let snapshot = self.default_scene.read().snapshot();
        self.current_scene = self.current_scene.restore_snapshot(snapshot);
    }

    pub fn load_scene(&mut self, scene: ReadOnlyRef<Scene>) {
        self.stop_simulation();

        let snapshot = scene.read().snapshot();
        self.current_scene = self.current_scene.restore_snapshot(snapshot);
        if let Some(asset_meta) = self.asset_registry.read().asset_meta_from_ref(&scene) {
            self.current_scene_meta = SceneMeta {
                file: asset_meta.path.clone(),
            };
        }
    }

    pub fn unload_current_scene(&mut self) {
        self.simulation_scene = None;
    }

    pub fn start_simulation(&mut self) {
        if self.simulation_scene.is_none() {
            let snapshot = self.current_scene.snapshot();
            self.simulation_scene = Some(self.current_scene.restore_snapshot(snapshot));
        }

        self.simulation_running = true;
    }

    pub fn pause_simulation(&mut self) {
        self.simulation_running = false;
    }

    pub fn stop_simulation(&mut self) {
        self.simulation_scene = None;
        self.simulation_running = false;
    }

    pub fn prepare(&mut self) {
        self.simulation_scene_mut().prepare();
    }

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

    pub fn has_simulation_scene(&self) -> bool {
        self.simulation_scene.is_some()
    }

    pub fn is_simulating(&self) -> bool {
        self.simulation_running
    }

    pub fn simulation_scene(&self) -> &Scene {
        if let Some(scene) = &self.simulation_scene {
            return scene;
        }
        &self.current_scene
    }

    pub fn simulation_scene_mut(&mut self) -> &mut Scene {
        if let Some(scene) = &mut self.simulation_scene {
            return scene;
        }
        &mut self.current_scene
    }

    pub fn current_scene_meta(&self) -> &SceneMeta {
        &self.current_scene_meta
    }

    pub fn current_scene(&self) -> &Scene {
        &self.current_scene
    }

    pub fn current_scene_mut(&mut self) -> &mut Scene {
        &mut self.current_scene
    }
}
