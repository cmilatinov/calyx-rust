use std::collections::HashSet;
use std::io::BufWriter;

use egui::Ui;
use egui::Vec2;

use engine::egui::{include_image, Button, Color32, Rounding, Sense};
use engine::indextree::NodeId;
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
                let mut parent: Option<NodeId> = None;
                if let Some(selected) = selection.clone() {
                    match selected {
                        EditorSelection::Entity(set) => {
                            if let Some(s) = set.iter().last() {
                                parent = Some(*s);
                            }
                        }
                        EditorSelection::Asset(_) => {}
                    }
                }
                SceneManager::get_mut()
                    .get_scene_mut()
                    .create_entity(None, parent);
            }
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Filter by name"));
        });

        for root_node in SceneManager::get().get_scene().root_entities() {
            self.render_scene_node(
                SceneManager::get().get_scene(),
                &app_state.selection,
                &mut selection,
                ui,
                *root_node,
            );
        }
        app_state.selection = selection;
    }
}

impl PanelSceneHierarchy {
    fn render_scene_node(
        &self,
        scene: &Scene,
        selected: &Option<EditorSelection>,
        selection: &mut Option<EditorSelection>,
        ui: &mut Ui,
        node_id: NodeId,
    ) {
        let children: Vec<NodeId> = scene.get_children(node_id).collect();

        let is_selected = if let Some(EditorSelection::Entity(set)) = selected {
            set.contains(&node_id)
        } else {
            false
        };

        if !children.is_empty() {
            let collapsing_id = ui.make_persistent_id(node_id);
            egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                collapsing_id,
                false,
            )
            .show_header(ui, |ui| {
                self.show_selectable_label(scene, is_selected, selection, ui, node_id);
            })
            .body(|ui| {
                for child_node in children {
                    self.render_scene_node(scene, selected, selection, ui, child_node)
                }
            });
        } else {
            ui.horizontal(|ui| {
                ui.add_space(BASE_FONT_SIZE + 2.0);
                self.show_selectable_label(scene, is_selected, selection, ui, node_id);
            });
        }
    }

    fn show_selectable_label(
        &self,
        scene: &Scene,
        is_selected: bool,
        selection: &mut Option<EditorSelection>,
        ui: &mut Ui,
        node_id: NodeId,
    ) {
        let svg = include_image!("../../../resources/icons/body_dark.png");
        let image =
            egui::Image::new(svg).fit_to_exact_size(Vec2::new(BASE_FONT_SIZE, BASE_FONT_SIZE));
        let res = ui.add(
            Button::image_and_text(image, scene.get_entity_name(node_id))
                .selected(is_selected)
                .fill(if is_selected {
                    ui.visuals().selection.bg_fill
                } else {
                    Color32::TRANSPARENT
                })
                .rounding(Rounding::ZERO)
                .sense(Sense::click()),
        );

        if res.clicked() || res.secondary_clicked() {
            let mut set = HashSet::new();
            set.insert(node_id);
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
                    let res = std::fs::OpenOptions::new()
                        .create(true)
                        .write(true)
                        .truncate(true)
                        .open(path);

                    if let Ok(file) = res {
                        let prefab = scene.create_prefab(node_id);

                        let writer = BufWriter::new(file);
                        serde_json::to_writer_pretty(writer, &prefab).unwrap();
                    }

                    ui.close_menu();
                }
            }
        });
    }
}
