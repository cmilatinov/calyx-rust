use std::collections::HashSet;
use std::io::BufWriter;

use egui::Ui;
use egui::Vec2;

use engine::egui::{include_image, Button, Color32, Rounding, Sense};
use engine::scene::GameObject;
use engine::scene::{Scene, SceneManager};
use engine::*;

use crate::panel::Panel;
use crate::{EditorAppState, EditorSelection, BASE_FONT_SIZE};

#[derive(Default)]
pub struct PanelSceneHierarchy {
    search: String,
}

impl Panel for PanelSceneHierarchy {
    fn name() -> &'static str {
        "Scene Hierarchy"
    }

    fn ui(&mut self, ui: &mut Ui) {
        let mut app_state = EditorAppState::get_mut();
        let mut selection = app_state.selection.clone();

        ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
            if ui.button("+").clicked() {
                let mut parent: Option<GameObject> = None;
                if let Some(selected) = selection.clone() {
                    match selected {
                        EditorSelection::Entity(set) => {
                            if let Some(id) = set.iter().last().copied() {
                                parent = SceneManager::get()
                                    .simulation_scene()
                                    .get_game_object_by_uuid(id);
                            }
                        }
                        EditorSelection::Asset(_) => {}
                    }
                }
                SceneManager::get_mut()
                    .simulation_scene_mut()
                    .create_game_object(None, parent);
            }
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Filter by name"));
        });

        let mut scene_manager = SceneManager::get_mut();
        let scene = scene_manager.simulation_scene_mut();
        for root_object in scene.root_objects().clone() {
            self.render_scene_node(scene, &app_state.selection, &mut selection, ui, root_object);
        }
        app_state.selection = selection;
    }

    fn context_menu(&mut self, ui: &mut Ui) {
        if ui.button("Test").clicked() {
            ui.close_menu();
        }
    }
}

impl PanelSceneHierarchy {
    fn render_scene_node(
        &self,
        scene: &mut Scene,
        selected: &Option<EditorSelection>,
        selection: &mut Option<EditorSelection>,
        ui: &mut Ui,
        game_object: GameObject,
    ) {
        let id = scene.get_game_object_uuid(game_object);
        let children: Vec<GameObject> = scene.get_children(game_object).collect();

        let is_selected = if let Some(EditorSelection::Entity(set)) = selected {
            set.contains(&id)
        } else {
            false
        };

        if !children.is_empty() {
            let collapsing_id = ui.make_persistent_id(id);
            egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                collapsing_id,
                false,
            )
            .show_header(ui, |ui| {
                self.show_selectable_label(scene, is_selected, selection, ui, game_object)
            })
            .body(|ui| {
                for child_node in children {
                    self.render_scene_node(scene, selected, selection, ui, child_node)
                }
            });
        } else {
            ui.horizontal(|ui| {
                ui.add_space(BASE_FONT_SIZE + 2.0);
                self.show_selectable_label(scene, is_selected, selection, ui, game_object);
            });
        }
    }

    fn show_selectable_label(
        &self,
        scene: &mut Scene,
        is_selected: bool,
        selection: &mut Option<EditorSelection>,
        ui: &mut Ui,
        game_object: GameObject,
    ) {
        let svg = include_image!("../../../resources/icons/body_dark.png");
        let image =
            egui::Image::new(svg).fit_to_exact_size(Vec2::new(BASE_FONT_SIZE, BASE_FONT_SIZE));
        let res = ui.add(
            Button::image_and_text(image, scene.get_game_object_name(game_object))
                .selected(is_selected)
                .fill(if is_selected {
                    ui.visuals().selection.bg_fill
                } else {
                    Color32::TRANSPARENT
                })
                .rounding(Rounding::ZERO)
                .sense(Sense::click_and_drag()),
        );

        if res.clicked() || res.secondary_clicked() {
            let mut set = HashSet::new();
            set.insert(scene.get_game_object_uuid(game_object));
            *selection = if is_selected {
                None
            } else {
                Some(EditorSelection::Entity(set))
            };
        }

        res.context_menu(|ui| {
            if ui.button("Save as prefab").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("file_name.cxprefab")
                    .add_filter("cxprefab", &["cxprefab"])
                    .save_file()
                {
                    if let Ok(file) = std::fs::OpenOptions::new()
                        .create(true)
                        .write(true)
                        .truncate(true)
                        .open(path)
                    {
                        let prefab = scene.create_prefab(game_object);
                        let writer = BufWriter::new(file);
                        serde_json::to_writer_pretty(writer, &prefab).unwrap();
                    }
                    ui.close_menu();
                }
            }
            if ui.button("Delete").clicked() {
                scene.delete_game_object(game_object);
                ui.close_menu();
            }
            if ui.button("New Game Object").clicked() {
                scene.create_game_object(None, Some(game_object));
                ui.close_menu();
            }
        });
    }
}
