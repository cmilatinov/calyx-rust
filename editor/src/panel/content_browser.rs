use std::fs::{DirEntry, ReadDir};
use std::path::PathBuf;
use std::{fs, io};

use engine::assets::AssetRegistry;
use engine::egui;
use engine::egui::text::LayoutJob;
use engine::egui::{
    include_image, Button, Color32, ImageSource, Margin, Response, Rounding, Sense, TextFormat, Ui,
    Vec2,
};
use engine::egui_dock::{TabBodyStyle, TabStyle};
use engine::relative_path::PathExt;

use crate::panel::Panel;
use crate::widgets::FileButton;
use crate::BASE_FONT_SIZE;

pub struct PanelContentBrowser {
    selected_folder: PathBuf,
    selected_file: Option<PathBuf>,
}

impl PanelContentBrowser {
    fn render_directory(&mut self, ui: &mut Ui, entry: DirEntry, children: io::Result<ReadDir>) {
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if !is_dir {
            return;
        }

        let curr_path = entry.path();
        let path = curr_path.to_str().unwrap().to_string();
        let collapsing_id = ui.make_persistent_id(path.clone());
        let is_selected = self.selected_folder == curr_path;
        let render_node = |ui: &mut Ui| {
            let svg = include_image!("../../../resources/icons/folder_dark.png");
            let image =
                egui::Image::new(svg).fit_to_exact_size(Vec2::new(BASE_FONT_SIZE, BASE_FONT_SIZE));
            let res = ui.add(
                Button::image_and_text(image, curr_path.file_name().unwrap().to_str().unwrap())
                    .selected(is_selected)
                    .fill(if is_selected {
                        ui.visuals().selection.bg_fill
                    } else {
                        Color32::TRANSPARENT
                    })
                    .rounding(Rounding::ZERO)
                    .sense(Sense::click()),
            );
            if res.clicked() {
                self.selected_folder = if is_selected {
                    AssetRegistry::get().root_path().clone()
                } else {
                    curr_path
                };
                self.selected_file = None;
            }
        };

        let child_entries: Vec<DirEntry> = children
            .map(|nodes| {
                nodes
                    .flatten()
                    .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                    .collect()
            })
            .unwrap_or(vec![]);
        let has_child_dir = child_entries.len() > 0;
        if has_child_dir {
            egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                collapsing_id,
                false,
            )
            .show_header(ui, render_node)
            .body(|ui| {
                for child in child_entries {
                    let path = child.path();
                    self.render_directory(ui, child, fs::read_dir(path));
                }
            });
        } else {
            ui.horizontal(|ui| {
                ui.allocate_exact_size(
                    Vec2::new(ui.spacing().icon_width, 0.0),
                    Sense {
                        click: false,
                        drag: false,
                        focusable: false,
                    },
                );
                render_node(ui);
            });
        }
    }
    fn render_file_button<'a>(
        ui: &'a mut Ui,
        name: &'a str,
        image_src: impl Into<ImageSource<'a>>,
        image_size: Vec2,
        image_spacing: f32,
        padding: Vec2,
        selected: bool,
    ) -> Response {
        let image = egui::Image::new(image_src).fit_to_exact_size(image_size);
        let mut job = LayoutJob::single_section(String::from(name), TextFormat::default());
        job.wrap.break_anywhere = true;
        job.wrap.overflow_character = Some('…');
        job.wrap.max_width = image_size.x;
        job.wrap.max_rows = 1;
        let button = FileButton {
            image,
            image_size,
            image_spacing,
            text: job.into(),
            padding,
            selected,
        };
        ui.add(button)
    }
}

impl Panel for PanelContentBrowser {
    fn name() -> &'static str {
        "Content Browser"
    }

    fn ui(&mut self, ui: &mut Ui) {
        egui::SidePanel::left("file_tree")
            .resizable(true)
            .show_inside(ui, |ui| {
                egui::ScrollArea::both().show(ui, |ui| {
                    let path = AssetRegistry::get().root_path().clone();
                    if let Ok(entries) = fs::read_dir(path) {
                        for entry in entries.flatten() {
                            let entry_path = entry.path();
                            self.render_directory(ui, entry, fs::read_dir(entry_path));
                        }
                    }
                });
            });

        let mut nodes = Vec::new();
        let fs = fs::read_dir(&self.selected_folder);
        if let Ok(entries) = fs {
            for entry in entries.flatten() {
                let curr_path = entry.path();
                nodes.push(curr_path);
            }
        }

        egui::TopBottomPanel::top("file_path")
            .exact_height(26.0)
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let mut root = AssetRegistry::get().root_path().clone();
                    let path = self.selected_folder.relative_to(root.clone()).unwrap();
                    let mut count = 0;
                    for (i, component) in path.components().enumerate() {
                        root.push(component.as_str());
                        if i == 0 {}
                        ui.label(">");
                        if ui.button(component.as_str()).clicked() {
                            if self.selected_folder != root {
                                self.selected_file = None;
                            }
                            self.selected_folder = root.clone();
                        }
                        count += 1;
                    }
                    if count == 0 {
                        ui.label(">");
                    }
                });
            });

        const ICON_SIZE: f32 = 75.0;
        const ICON_PADDING_X: f32 = 10.0;
        const ICON_PADDING_Y: f32 = 5.0;
        const ICON_SPACING: f32 = 10.0;
        const TOTAL_WIDTH: f32 = ICON_SIZE + ICON_PADDING_X * 2.0;
        let folder_image = include_image!("../../../resources/icons/folder_large.png");
        let file_image = include_image!("../../../resources/icons/body_dark_large.png");
        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::ScrollArea::both().show(ui, |ui| {
                let width = ui.available_width();
                let mut num_nodes_per_row = (width / TOTAL_WIDTH) as usize;
                if num_nodes_per_row == 0 {
                    num_nodes_per_row = 1;
                }
                egui::Grid::new("content_browser").show(ui, |ui| {
                    for (i, node) in nodes.iter().enumerate() {
                        let is_dir = node.is_dir();
                        let is_selected = if let Some(selection) = self.selected_file.as_ref() {
                            *selection == *node
                        } else {
                            false
                        };
                        let res = PanelContentBrowser::render_file_button(
                            ui,
                            node.file_name().unwrap().to_str().unwrap(),
                            if is_dir {
                                folder_image.clone()
                            } else {
                                file_image.clone()
                            },
                            Vec2::splat(ICON_SIZE),
                            ICON_SPACING,
                            Vec2::new(ICON_PADDING_X, ICON_PADDING_Y),
                            is_selected,
                        );
                        if res.clicked() {
                            self.selected_file = Some(node.clone());
                        }
                        if res.double_clicked() && is_dir {
                            self.selected_folder = node.clone();
                            self.selected_file = None;
                        }
                        if (i + 1) % num_nodes_per_row == 0 {
                            ui.add_space(ui.available_width());
                            ui.end_row();
                        }
                    }
                });
                ui.add_space(15.0);
            });
        });
    }

    fn tab_style_override(&self, global_style: &TabStyle) -> Option<TabStyle> {
        Some(TabStyle {
            tab_body: TabBodyStyle {
                inner_margin: Margin::ZERO,
                ..global_style.tab_body.clone()
            },
            ..global_style.clone()
        })
    }
}

impl Default for PanelContentBrowser {
    fn default() -> Self {
        PanelContentBrowser {
            selected_folder: AssetRegistry::get().root_path().clone(),
            selected_file: None,
        }
    }
}
