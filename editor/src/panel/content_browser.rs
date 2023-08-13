use engine::*;
use egui::Ui;
use std::fs;
use std::path::Path;
use crate::panel::Panel;

pub struct PanelContentBrowser;
impl PanelContentBrowser {
    fn render_directory(&self, ui: &mut egui::Ui, path: &Path) {
        // Attempt to read the directory
        match fs::read_dir(path) {
            Ok(entries) => {
                for entry in entries {
                    if let Ok(entry) = entry {
                        let curr_path = entry.path();
                        // If the entry is a directory, create a collapsing header
                        if entry.file_type().unwrap().is_dir() {
                            let dir_name = curr_path.file_name().unwrap().to_str().unwrap();

                            // Use collapsing header for directories
                            ui.collapsing(dir_name, |ui| {
                                self.render_directory(ui, &entry.path());
                            });
                        } else {
                            // Simply list the file if it's not a directory
                            let file_name = curr_path.file_name().unwrap().to_str().unwrap();
                            ui.label(file_name);
                        }
                    }
                }
            },
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
            .width_range(80.0..=250.0)
            .show_inside(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("Left Panel");
                });
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.render_directory(ui, Path::new("/run/media/rubens/ssd/projects/calyx-rust/"));
                });
            });

        egui::CentralPanel::default()
            .show_inside(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("Central Panel");
                });
            });
    }
}