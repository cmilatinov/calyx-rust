use project::Project;
use dirs::config_dir;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;
use std::vec::Vec;
use egui::text::LayoutJob;
use egui::TextFormat;
use egui_modal::Modal;

#[derive(Default)]
pub struct NewProjectForm {
    name: String,
    root_directory: String
}

#[derive(Default)]
pub struct LauncherApp {
    search: String,
    projects: Vec<Project>,
    new_project_form: NewProjectForm
}

impl LauncherApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let mut app = LauncherApp::default();
        app.load_projects();
        app
    }
}

impl eframe::App for LauncherApp {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let modal = Modal::new(ctx, "my_modal");

            modal.show(|ui| {
                modal.frame(ui, |ui| {
                    ui.heading("Create a Project");

                    ui.add_space(10.0);

                    ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                        ui.label("Project Name");
                        ui.add(egui::TextEdit::singleline(&mut self.new_project_form.name));
                    });

                    ui.add_space(10.0);

                    ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                        ui.label("Directory");
                        ui.add(egui::TextEdit::singleline(&mut self.new_project_form.root_directory));

                        if ui.button("🖹").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                self.new_project_form.root_directory = path.display().to_string();
                            }
                        }
                    });

                    ui.add_space(10.0);

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                        if ui.button("Close").clicked() {
                            modal.close();
                        }

                        if ui.button("Create").clicked() {
                            let new_project = Project::generate(self.new_project_form.name.clone(), Some(self.new_project_form.root_directory.clone()));
                            self.save_project(new_project);
                            modal.close();
                        }
                    });
                });
            });

            ui.add_space(10.0);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search Here"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    if ui.button("New Project").clicked() {
                        modal.open();
                    }

                    if ui.button("Open").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            launch_editor(path.display().to_string());
                        }
                    }
                });
            });
            ui.add_space(10.0);
            
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::Center), |ui| {
                    for project in &self.projects {
                        if !self.search.is_empty() && !project.name().contains(&self.search) {
                            continue;
                        }

                        let mut layout_job = LayoutJob::default();
                        layout_job.append(&*(project.name().to_string() + "\n"),
                                          0.0,
                                          TextFormat::default());
                        layout_job.append(project.root_directory().to_str().unwrap(),
                                          00.0,
                                          TextFormat::default());

                        if ui.add_sized([120., 40.],
                                        egui::Button::new(layout_job)).clicked() {
                            launch_editor(String::from(project.root_directory().to_str().unwrap()));
                        }
                    }
                });
            });
        });
    }
}

impl LauncherApp {
    pub fn save_project(&mut self, curr_project: Project) {
        if let Some(path) = get_config_path() {
            // Make sure the directory exists
            if let Some(parent) = path.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent).unwrap();
                }
            }

            self.projects.push(curr_project);

            // Save the projects to the JSON file
            let json_data = serde_json::to_string_pretty(&self.projects).unwrap();
            let mut file = File::create(path).unwrap();
            file.write_all(json_data.as_bytes()).unwrap();
        }
    }

    pub fn load_projects(&mut self) {
        if let Some(path) = get_config_path() {
            if path.exists() {
                let mut file = File::open(path).unwrap();
                let mut contents = String::new();
                file.read_to_string(&mut contents).unwrap();
                self.projects = serde_json::from_str(&contents).unwrap();
                self.projects.retain(|p| p.root_directory().exists());
            }
        }
    }
}

fn get_config_path() -> Option<PathBuf> {
    if let Some(mut config_path) = config_dir() {
        config_path.push("calyx");
        config_path.push("recent_projects.json");
        Some(config_path)
    } else {
        None
    }
}

pub fn launch_editor(root_directory: String) {
    let mut dir = std::env::current_dir().unwrap().clone();
    dir.push("target/debug/editor");
    Command::new(dir)
        .arg(root_directory)
        .spawn()
        .expect("Failed to start editor.");

    std::process::exit(0);
}
