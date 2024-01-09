use crate as engine;

use std::ops::DerefMut;
use crate::utils::singleton_with_init;
use crate::scene::Scene;
use egui::Ui;
use std::path::PathBuf;
use crate::assets::Asset;

#[derive(Default)]
pub struct SceneManager {
    simulation_running: bool,
    current_scene: Scene,
    simulation_scene: Option<Scene>
}

impl SceneManager {
    fn load_scene(&mut self, scene_file: PathBuf) {
        self.stop_simulation();
        if let Ok(loaded_scene) = Scene::from_file(&scene_file) {
            self.current_scene = loaded_scene;
        }
    }

    fn unload_current_scene(&mut self) {
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
}

singleton_with_init!(SceneManager);
