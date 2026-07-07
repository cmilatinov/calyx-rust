use crate::panel::Panel;
use crate::selection::{Selection, SelectionType};
use crate::widgets::{
    FileButton, ThumbnailPriority, ThumbnailRequest, ThumbnailStatus, THUMBNAIL_MAX_SIZE,
    THUMBNAIL_MIN_SIZE,
};
use crate::{icons, EditorAppState};
use egui::load::SizedTexture;
use egui::text::LayoutJob;
use egui::{
    Align, FontFamily, FontId, Frame, ImageSource, Layout, Margin, Rect, Response, Sense, Slider,
    TextFormat, Ui, Vec2,
};
use engine::assets::animation_graph::AnimationGraph;
use re_ui::list_item::ShowCollapsingResponse;
use relative_path::PathExt;
use std::any::Any;
use std::fs::{DirEntry, OpenOptions, ReadDir};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::{fs, io};

const CONTENT_BROWSER_THUMBNAIL_DEFAULT_SIZE_PX: f32 = THUMBNAIL_MIN_SIZE as f32;

pub struct PanelContentBrowser {
    selected_folder: PathBuf,
    selected_file: Option<PathBuf>,
    thumbnail_size_px: f32,
}

impl PanelContentBrowser {
    pub fn new(root_path: impl Into<PathBuf>) -> Self {
        PanelContentBrowser {
            selected_folder: root_path.into(),
            selected_file: None,
            thumbnail_size_px: CONTENT_BROWSER_THUMBNAIL_DEFAULT_SIZE_PX,
        }
    }
}

impl Panel for PanelContentBrowser {
    fn name() -> &'static str {
        "Content Browser"
    }

    fn ui(&mut self, ui: &mut Ui, state: &mut EditorAppState) {
        let root_path = state
            .game
            .assets
            .registries
            .assets
            .read()
            .root_path()
            .clone();

        egui::SidePanel::left("file_tree")
            .resizable(true)
            .show_inside(ui, |ui| {
                egui::ScrollArea::both().show(ui, |ui| {
                    re_ui::list_item::list_item_scope(ui, "file_tree_scope", |ui| {
                        if let Ok(entries) = fs::read_dir(&root_path) {
                            for entry in entries.flatten() {
                                let entry_path = entry.path();
                                self.render_directory(ui, state, entry, fs::read_dir(entry_path));
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
                if curr_path.is_file()
                    && curr_path.extension().and_then(|e| e.to_str()) == Some("meta")
                {
                    continue;
                }
                nodes.push(curr_path);
            }
        }

        egui::TopBottomPanel::top("file_path")
            .exact_height(30.0)
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let slider_width = 128.0;
                    let path_width = (ui.available_width() - slider_width - 8.0).max(0.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(path_width, 24.0),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            let mut root = root_path;
                            let path = self.selected_folder.relative_to(root.clone()).unwrap();
                            if ui.button(">").clicked() {
                                self.set_selected_folder(&mut state.selection, root.clone());
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
                                    self.set_selected_folder(&mut state.selection, root.clone());
                                }
                                component = iterator.next();
                                if component.is_some() {
                                    ui.label(">");
                                }
                            }
                        },
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_sized(
                            Vec2::new(slider_width, 18.0),
                            Slider::new(
                                &mut self.thumbnail_size_px,
                                THUMBNAIL_MIN_SIZE as f32..=THUMBNAIL_MAX_SIZE as f32,
                            )
                            .show_value(false),
                        )
                        .on_hover_text("Thumbnail size");
                    });
                    self.thumbnail_size_px = self
                        .thumbnail_size_px
                        .clamp(THUMBNAIL_MIN_SIZE as f32, THUMBNAIL_MAX_SIZE as f32);
                });
            });

        let pixels_per_point = ui.pixels_per_point().max(f32::EPSILON);
        let icon_size_px = self.thumbnail_size_px.round();
        let icon_size = icon_size_px / pixels_per_point;
        let icon_padding_x = (icon_size * 0.08).clamp(8.0, 18.0);
        let icon_padding_y = 5.0;
        let icon_spacing = 10.0;
        let total_width = icon_size + icon_padding_x * 2.0;
        let folder_image = egui::include_image!("../../../resources/icons/folder_large.png");
        let file_image = egui::include_image!("../../../resources/icons/body_dark_large.png");
        egui::CentralPanel::default()
            .frame(Frame {
                inner_margin: Margin::same(3),
                ..Frame::central_panel(ui.style())
            })
            .show_inside(ui, |ui| {
                self.handle_thumbnail_zoom_input(ui);
                egui::ScrollArea::both().show(ui, |ui| {
                    let width = ui.available_width();
                    let spacing = ui.style().spacing.item_spacing;
                    ui.style_mut().spacing.item_spacing = Vec2::ZERO;
                    let num_nodes_per_row = ((width / total_width) as usize).max(1);
                    ui.horizontal_wrapped(|ui| {
                        for (idx, node) in nodes.iter().enumerate() {
                            let is_dir = node.is_dir();
                            let is_selected = self.is_selected(state, node, is_dir);
                            let image_size = Vec2::splat(icon_size);
                            let image_src = if is_dir {
                                folder_image.clone()
                            } else {
                                self.asset_thumbnail_image(state, node, image_size)
                                    .unwrap_or_else(|| file_image.clone())
                            };
                            let res = PanelContentBrowser::render_file_button(
                                ui,
                                node.file_name().unwrap().to_str().unwrap(),
                                image_src,
                                image_size,
                                icon_spacing,
                                Vec2::new(icon_padding_x, icon_padding_y),
                                is_selected,
                            );
                            if res.clicked() || res.secondary_clicked() {
                                self.set_selected_file(state, node.clone());
                            }
                            if is_selected && is_dir && res.double_clicked() {
                                self.set_selected_folder(&mut state.selection, node.clone());
                            }
                            if !is_dir {
                                self.asset_context_menu(state, &res, node);
                            }
                            if idx % num_nodes_per_row == num_nodes_per_row - 1 {
                                let remaining_width =
                                    width - num_nodes_per_row as f32 * total_width - 1.0;
                                if remaining_width > 0.0 {
                                    let (_, rect) = ui.allocate_space(Vec2::new(
                                        remaining_width,
                                        ui.available_height(),
                                    ));
                                    self.empty_space_interaction(ui, rect);
                                }
                            }
                        }
                        let remaining_width =
                            width - (nodes.len() % num_nodes_per_row) as f32 * total_width - 1.0;
                        if remaining_width > 0.0 {
                            let (_, rect) = ui
                                .allocate_space(Vec2::new(remaining_width, ui.available_height()));
                            self.empty_space_interaction(ui, rect);
                        }
                    });
                    ui.style_mut().spacing.item_spacing = spacing;
                    let (_, rect) = ui.allocate_space(ui.available_size());
                    self.empty_space_interaction(ui, rect);
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
    fn handle_thumbnail_zoom_input(&mut self, ui: &Ui) {
        if !ui.rect_contains_pointer(ui.max_rect()) {
            return;
        }

        let zoom_delta = ui.input(|input| {
            if input.modifiers.ctrl {
                input.zoom_delta()
            } else {
                1.0
            }
        });
        if (zoom_delta - 1.0).abs() <= f32::EPSILON {
            return;
        }

        self.thumbnail_size_px = (self.thumbnail_size_px * zoom_delta)
            .clamp(THUMBNAIL_MIN_SIZE as f32, THUMBNAIL_MAX_SIZE as f32);
    }

    fn asset_thumbnail_image(
        &self,
        state: &mut EditorAppState,
        path: &Path,
        image_size: Vec2,
    ) -> Option<ImageSource<'static>> {
        let request = {
            let registry = state.game.assets.registries.assets.read();
            ThumbnailRequest::from_asset_path(&registry, path)
        }?;
        let key = request.key();
        let status = state.thumbnails.request(request, ThumbnailPriority::Normal);
        if !matches!(status, ThumbnailStatus::Ready) {
            return None;
        }
        state.thumbnails.texture_id(key).map(|id| {
            ImageSource::Texture(SizedTexture {
                id,
                size: image_size,
            })
        })
    }

    fn empty_space_interaction(&mut self, ui: &mut Ui, rect: Rect) {
        ui.allocate_rect(rect, Sense::click()).context_menu(|ui| {
            ui.menu_button("Create New", |ui| {
                if ui.button("Animation Graph").clicked() {
                    let mut path = self.selected_folder.clone();
                    path.push("untitled.cxanim");
                    if let Ok(file) = OpenOptions::new()
                        .create(true)
                        .write(true)
                        .truncate(true)
                        .open(path)
                    {
                        let writer = BufWriter::new(file);
                        let _ = serde_json::to_writer_pretty(writer, &AnimationGraph::default());
                    }
                    ui.close_menu();
                }
            });
        });
    }

    fn asset_context_menu(&mut self, state: &mut EditorAppState, response: &Response, path: &Path) {
        let (asset_id, type_uuid) = {
            let registry = state.game.assets.registries.assets.read();
            let asset_id = registry.asset_id_from_path(path);
            let type_uuid = path
                .extension()
                .and_then(|e| e.to_str())
                .and_then(|ext| registry.asset_type_uuid_from_ext(ext));
            (asset_id, type_uuid)
        };
        let Some(asset_id) = asset_id else {
            return;
        };

        let inspector_registry = &state.inspector_registry;
        let game = &mut state.game;
        response.context_menu(|ui| {
            let Some(inspector) = type_uuid
                .and_then(|type_uuid| inspector_registry.asset_inspector_lookup(type_uuid))
            else {
                ui.label("No actions available");
                return;
            };
            if inspector.has_context_menu() {
                inspector.show_context_menu(ui, game, asset_id);
            } else {
                ui.label("No actions available");
            }
        });
    }

    fn render_directory(
        &mut self,
        ui: &mut Ui,
        state: &mut EditorAppState,
        entry: DirEntry,
        children: io::Result<ReadDir>,
    ) {
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
                        self.render_directory(ui, state, child, fs::read_dir(path));
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
                state
                    .game
                    .assets
                    .registries
                    .assets
                    .read()
                    .root_path()
                    .clone()
            } else {
                curr_path
            };
            self.selected_file = None;
            state.selection = Selection::none();
        }
    }

    fn render_file_button<'a>(
        ui: &'a mut Ui,
        name: &'a str,
        image_src: impl Into<egui::ImageSource<'a>>,
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

    fn set_selected_folder(&mut self, selection: &mut Selection, path: PathBuf) {
        if path != self.selected_folder {
            self.selected_folder = path;
            self.selected_file = None;
            *selection = Selection::none();
        }
    }

    fn set_selected_file(&mut self, state: &mut EditorAppState, path: PathBuf) {
        state.selection = state
            .game
            .assets
            .registries
            .assets
            .read()
            .asset_id_from_path(&path)
            .map(|id| Selection::from_id(SelectionType::Asset, id))
            .unwrap_or_else(|| Selection::none());
        self.selected_file = Some(path);
    }

    fn is_selected(&self, state: &EditorAppState, path: &PathBuf, is_dir: bool) -> bool {
        if is_dir {
            if let Some(selection) = self.selected_file.as_ref() {
                *selection == *path
            } else {
                false
            }
        } else {
            state
                .game
                .assets
                .registries
                .assets
                .read()
                .asset_id_from_path(path)
                .map(|id| state.selection.contains(SelectionType::Asset, id))
                .unwrap_or(false)
        }
    }
}
