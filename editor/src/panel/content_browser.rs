use crate::inspector::inspector_registry::InspectorRegistry;
use crate::panel::Panel;
use crate::selection::{Selection, SelectionType};
use crate::widgets::FileButton;
use crate::{icons, EditorAppState};
use engine::assets::AssetRegistry;
use engine::egui;
use engine::egui::text::LayoutJob;
use engine::egui::{
    include_image, FontFamily, FontId, ImageSource, Response, TextFormat, Ui, Vec2,
};
use engine::relative_path::PathExt;
use re_ui::list_item::ShowCollapsingResponse;
use std::any::Any;
use std::fs::{DirEntry, ReadDir};
use std::path::PathBuf;
use std::{fs, io};

pub struct PanelContentBrowser {
    selected_folder: PathBuf,
    selected_file: Option<PathBuf>,
}

impl Default for PanelContentBrowser {
    fn default() -> Self {
        PanelContentBrowser {
            selected_folder: AssetRegistry::get().root_path().clone(),
            selected_file: None,
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
            .show_inside(ui, |ui| {
                egui::ScrollArea::both().show(ui, |ui| {
                    re_ui::list_item::list_item_scope(ui, "file_tree_scope", |ui| {
                        let path = AssetRegistry::get().root_path().clone();
                        if let Ok(entries) = fs::read_dir(path) {
                            for entry in entries.flatten() {
                                let entry_path = entry.path();
                                self.render_directory(ui, entry, fs::read_dir(entry_path));
                            }
                        }
                    });
                });
            });

        let mut nodes = Vec::new();
        let fs = fs::read_dir(&self.selected_folder);
        if let Ok(entries) = fs {
            for entry in entries.flatten() {
                let curr_path = entry.path();
                if curr_path.is_file() {
                    if let Some(ext) = curr_path.extension().and_then(|e| e.to_str()) {
                        if ext == "meta" {
                            continue;
                        }
                    }
                }
                nodes.push(curr_path);
            }
        }

        egui::TopBottomPanel::top("file_path")
            .exact_height(26.0)
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let mut root = AssetRegistry::get().root_path().clone();
                    let path = self.selected_folder.relative_to(root.clone()).unwrap();
                    if ui.button(">").clicked() {
                        self.set_selected_folder(AssetRegistry::get().root_path().clone());
                    }
                    let mut iterator = path.components();
                    let mut component = iterator.next();
                    loop {
                        if component.is_none() {
                            break;
                        }
                        let name = component.unwrap();
                        root.push(name.as_str());
                        if ui.button(name.as_str()).clicked() {
                            self.set_selected_folder(root.clone());
                        }
                        component = iterator.next();
                        if component.is_some() {
                            ui.label(">");
                        }
                    }
                });
            });

        const ICON_SIZE: f32 = 50.0;
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
                        let is_selected = self.is_selected(node, is_dir);
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
                        if res.clicked() || res.secondary_clicked() {
                            self.set_selected_file(node.clone());
                        }
                        if is_selected && is_dir && res.double_clicked() {
                            self.set_selected_folder(node.clone());
                        }
                        if (i + 1) % num_nodes_per_row == 0 {
                            ui.add_space(ui.available_width());
                            ui.end_row();
                        }
                        if !is_dir {
                            let ext = node
                                .extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or_default();
                            let registry = AssetRegistry::get();
                            let Some(type_id) = registry.asset_type_from_ext(ext) else {
                                return;
                            };
                            let Some(asset_id) = registry.asset_id_from_path(node) else {
                                return;
                            };
                            let registry = InspectorRegistry::get();
                            let Some(inspector) = registry.asset_inspector_lookup(type_id) else {
                                return;
                            };
                            if inspector.has_context_menu() {
                                res.context_menu(|ui| {
                                    inspector.show_context_menu(ui, asset_id);
                                });
                            }
                        }
                    }
                });
                let mut size = ui.available_size();
                size.y = size.y.max(15.0);
                ui.allocate_space(size);
            });
        });
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl PanelContentBrowser {
    fn render_directory(&mut self, ui: &mut Ui, entry: DirEntry, children: io::Result<ReadDir>) {
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if !is_dir {
            return;
        }

        let curr_path = entry.path();
        let path = curr_path.to_str().unwrap().to_string();
        let collapsing_id = ui.make_persistent_id(path);
        let is_selected = self.selected_folder == curr_path;
        let text = curr_path.file_name().unwrap().to_str().unwrap();

        let item = re_ui::list_item::ListItem::new()
            .draggable(true)
            .selected(is_selected);

        let child_entries: Vec<DirEntry> = children
            .map(|nodes| {
                nodes
                    .flatten()
                    .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                    .collect()
            })
            .unwrap_or_default();
        let response;
        if !child_entries.is_empty() {
            ShowCollapsingResponse {
                item_response: response,
                ..
            } = item.show_hierarchical_with_children(
                ui,
                collapsing_id,
                false,
                re_ui::list_item::LabelContent::new(text).with_icon(&icons::FOLDER),
                |ui| {
                    for child in child_entries {
                        let path = child.path();
                        self.render_directory(ui, child, fs::read_dir(path));
                    }
                },
            );
        } else {
            response = item.show_hierarchical(
                ui,
                re_ui::list_item::LabelContent::new(text).with_icon(&icons::FOLDER),
            );
        }

        if response.clicked() {
            self.selected_folder = if is_selected {
                AssetRegistry::get().root_path().clone()
            } else {
                curr_path
            };
            self.selected_file = None;
            EditorAppState::get_mut().selection = Selection::none();
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
        let mut format = TextFormat::default();
        format.font_id = FontId::new(11.0, FontFamily::Proportional);
        let mut job = LayoutJob::single_section(String::from(name), format);
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
        ui.add(button).on_hover_text_at_pointer(name)
    }

    fn set_selected_folder(&mut self, path: PathBuf) {
        if path != self.selected_folder {
            self.selected_folder = path;
            self.selected_file = None;
            EditorAppState::get_mut().selection = Selection::none();
        }
    }

    fn set_selected_file(&mut self, path: PathBuf) {
        // let root = AssetRegistry::get().root_path().clone();
        // let path = path.relative_to(root).unwrap().to;
        EditorAppState::get_mut().selection = AssetRegistry::get()
            .asset_id_from_path(&path)
            .map(|id| Selection::from_id(SelectionType::Asset, id))
            .unwrap_or_else(|| Selection::none());
        self.selected_file = Some(path);
    }

    fn is_selected(&self, path: &PathBuf, is_dir: bool) -> bool {
        if is_dir {
            if let Some(selection) = self.selected_file.as_ref() {
                *selection == *path
            } else {
                false
            }
        } else {
            AssetRegistry::get()
                .asset_id_from_path(path)
                .map(|id| {
                    EditorAppState::get()
                        .selection
                        .contains(SelectionType::Asset, id)
                })
                .unwrap_or(false)
        }
    }
}
