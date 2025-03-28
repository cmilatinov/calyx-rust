use crate::panel::Panel;
use crate::selection::{Selection, SelectionType};
use crate::widgets::FileButton;
use crate::{icons, EditorAppState};
use engine::assets::animation_graph::AnimationGraph;
use engine::egui::text::LayoutJob;
use engine::egui::{
    include_image, FontFamily, FontId, Frame, ImageSource, Margin, Rect, Response, Sense,
    TextFormat, Ui, Vec2,
};
use engine::relative_path::PathExt;
use engine::{egui, serde_json};
use re_ui::list_item::ShowCollapsingResponse;
use std::any::Any;
use std::fs::{DirEntry, OpenOptions, ReadDir};
use std::io::BufWriter;
use std::path::PathBuf;
use std::{fs, io};

pub struct PanelContentBrowser {
    selected_folder: PathBuf,
    selected_file: Option<PathBuf>,
}

impl PanelContentBrowser {
    pub fn new(root_path: impl Into<PathBuf>) -> Self {
        PanelContentBrowser {
            selected_folder: root_path.into(),
            selected_file: None,
        }
    }
}

impl Panel for PanelContentBrowser {
    fn name() -> &'static str {
        "Content Browser"
    }

    fn ui(&mut self, ui: &mut Ui, state: &mut EditorAppState) {
        let root_path = state.game.assets.asset_registry.read().root_path().clone();

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
            .exact_height(26.0)
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
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
                });
            });

        const ICON_SIZE: f32 = 50.0;
        const ICON_PADDING_X: f32 = 10.0;
        const ICON_PADDING_Y: f32 = 5.0;
        const ICON_SPACING: f32 = 10.0;
        const TOTAL_WIDTH: f32 = ICON_SIZE + ICON_PADDING_X * 2.0;
        let folder_image = include_image!("../../../resources/icons/folder_large.png");
        let file_image = include_image!("../../../resources/icons/body_dark_large.png");
        egui::CentralPanel::default()
            .frame(Frame {
                inner_margin: Margin::same(3),
                ..Frame::central_panel(ui.style())
            })
            .show_inside(ui, |ui| {
                egui::ScrollArea::both().show(ui, |ui| {
                    ui.set_clip_rect(ui.max_rect().expand(3.0));
                    let width = ui.available_width();
                    let spacing = ui.style().spacing.item_spacing;
                    ui.style_mut().spacing.item_spacing = Vec2::ZERO;
                    let num_nodes_per_row = ((width / TOTAL_WIDTH) as usize).max(1);
                    ui.horizontal_wrapped(|ui| {
                        for (idx, node) in nodes.iter().enumerate() {
                            let is_dir = node.is_dir();
                            let is_selected = self.is_selected(state, node, is_dir);
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
                                self.set_selected_file(state, node.clone());
                            }
                            if is_selected && is_dir && res.double_clicked() {
                                self.set_selected_folder(&mut state.selection, node.clone());
                            }
                            if idx % num_nodes_per_row == num_nodes_per_row - 1 {
                                let remaining_width =
                                    width - num_nodes_per_row as f32 * TOTAL_WIDTH - 1.0;
                                if remaining_width > 0.0 {
                                    let (_, rect) = ui.allocate_space(Vec2::new(
                                        remaining_width,
                                        ui.available_height(),
                                    ));
                                    self.empty_space_interaction(ui, rect);
                                }
                            }
                            if !is_dir {
                                let ext = node
                                    .extension()
                                    .and_then(|e| e.to_str())
                                    .unwrap_or_default();
                                let registry = state.game.assets.asset_registry.read();
                                let Some((_, type_uuid, _)) = registry.asset_type_from_ext(ext)
                                else {
                                    continue;
                                };
                                let Some(asset_id) = registry.asset_id_from_path(node) else {
                                    continue;
                                };
                                drop(registry);
                                let Some(inspector) =
                                    state.inspector_registry.asset_inspector_lookup(type_uuid)
                                else {
                                    continue;
                                };
                                if inspector.has_context_menu() {
                                    res.context_menu(|ui| {
                                        inspector.show_context_menu(ui, &mut state.game, asset_id);
                                    });
                                }
                            }
                        }
                        let remaining_width =
                            width - (nodes.len() % num_nodes_per_row) as f32 * TOTAL_WIDTH - 1.0;
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
                state.game.assets.asset_registry.read().root_path().clone()
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
            .asset_registry
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
                .asset_registry
                .read()
                .asset_id_from_path(path)
                .map(|id| state.selection.contains(SelectionType::Asset, id))
                .unwrap_or(false)
        }
    }
}
