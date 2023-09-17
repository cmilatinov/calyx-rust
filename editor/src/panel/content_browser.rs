use engine::assets::AssetRegistry;
use engine::egui;
use engine::egui::Ui;
use std::fs;
use std::path::Path;

use crate::panel::Panel;

pub struct PanelContentBrowser {
    selected_folder: String,
}

impl PanelContentBrowser {
    fn render_directory(&mut self, ui: &mut Ui, path: &Path) {
        match fs::read_dir(path) {
            Ok(entries) => {
                for entry in entries {
                    if let Ok(entry) = entry {
                        let curr_path = entry.path();

                        if entry.file_type().unwrap().is_dir() {
                            let path = curr_path.to_str().unwrap().to_string();
                            let collapsing_id = ui.make_persistent_id(path.clone());
                            let is_selected = path == self.selected_folder;

                            egui::collapsing_header::CollapsingState::load_with_default_open(
                                ui.ctx(),
                                collapsing_id,
                                false,
                            )
                            .show_header(ui, |ui| {
                                let res = ui.selectable_label(
                                    is_selected,
                                    curr_path.file_name().unwrap().to_str().unwrap(),
                                );

                                if res.clicked() {
                                    self.selected_folder = path;
                                }
                            })
                            .body(|ui| self.render_directory(ui, &entry.path()));
                        }
                    }
                }
            }
            Err(e) => {
                ui.label(format!("Failed to read directory: {}", e));
            }
        }
    }
}

impl Panel for PanelContentBrowser {
    fn name() -> &'static str {
        "Content Browser"
    }

    fn ui(&mut self, ui: &mut Ui) {
        egui::SidePanel::left("file_tree")
            .resizable(true)
            .default_width(150.0)
            .width_range(100.0..=250.0)
            .show_inside(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("Assets");
                });
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.render_directory(ui, Path::new(AssetRegistry::get().root_path()));
                });
            });

        let mut nodes = Vec::new();
        let fs = fs::read_dir(self.selected_folder.to_string());
        if let Ok(entries) = fs {
            for res in entries {
                if let Ok(entry) = res {
                    let curr_path = entry.path();
                    nodes.push(curr_path.file_name().unwrap().to_str().unwrap().to_string());
                }
            }
        }

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Content");
            });
            egui::Grid::new("content_browser")
                .striped(true)
                .show(ui, |ui| {
                    for node in nodes {
                        ui.label(node);
                    }
                });
        });
    }
}

impl Default for PanelContentBrowser {
    fn default() -> Self {
        PanelContentBrowser {
            selected_folder: AssetRegistry::get().root_path().to_string(),
        }
    }
}
