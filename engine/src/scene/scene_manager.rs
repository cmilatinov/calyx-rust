use crate as engine;

use crate::assets::Asset;
use crate::scene::Scene;
use crate::utils::singleton_with_init;
use egui::Ui;
use std::ops::DerefMut;
use std::path::PathBuf;

#[derive(Default)]
pub struct SceneManager {
    simulation_running: bool,
    current_scene: Scene,
    simulation_scene: Option<Scene>,
}

impl SceneManager {
    pub fn load_scene(&mut self, scene_file: PathBuf) {
        self.stop_simulation();
        if let Ok(loaded_scene) = Scene::from_file(&scene_file) {
            self.current_scene = loaded_scene;
        }
    }

    pub fn unload_current_scene(&mut self) {
        self.simulation_scene = None;
    }

    pub fn start_simulation(&mut self) {
        if self.simulation_scene.is_none() {
            self.simulation_scene = Some(self.current_scene.clone());
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

    pub fn update(&mut self, ui: &mut Ui) {
        if !self.simulation_running {
            return;
        }

        if let Some(scene) = &self.simulation_scene {
            scene.update(ui);
        }
    }

    pub fn is_simulating(&self) -> bool {
        self.simulation_running
    }

    pub fn get_scene(&self) -> &Scene {
        if let Some(scene) = &self.simulation_scene {
            return scene
        }
        &self.current_scene
    }

    pub fn get_scene_mut(&mut self) -> &mut Scene {
        if let Some(scene) = &mut self.simulation_scene {
            return scene
        }
        &mut self.current_scene
    }
}

singleton_with_init!(SceneManager);
