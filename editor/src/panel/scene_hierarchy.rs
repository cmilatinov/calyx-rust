use crate::panel::Panel;
use crate::{icons, EditorAppState, Selection, SelectionType};
use egui::Ui;
use engine::component::ComponentID;
use engine::egui::{Color32, Response};
use engine::scene::{GameObject, SiblingDir};
use engine::scene::{Scene, SceneManager};
use engine::uuid::Uuid;
use engine::*;
use re_ui::drag_and_drop::{DropTarget, ItemKind};
use re_ui::{DesignTokens, UiExt};
use std::any::Any;
use std::io::BufWriter;
use std::sync::mpsc::{Receiver, Sender};

enum Command {
    SetTargetContainer(GameObject),
}

pub struct PanelSceneHierarchy {
    target_container: Option<GameObject>,
    sender: Sender<Command>,
    receiver: Receiver<Command>,
}

impl Default for PanelSceneHierarchy {
    fn default() -> Self {
        let (sender, receiver) = std::sync::mpsc::channel();
        Self {
            target_container: Default::default(),
            sender,
            receiver,
        }
    }
}

impl Panel for PanelSceneHierarchy {
    fn name() -> &'static str {
        "Scene Hierarchy"
    }

    fn ui(&mut self, ui: &mut Ui) {
        let mut app_state = EditorAppState::get_mut();

        egui::ScrollArea::both()
            .id_salt("scene_scroll_area")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame {
                    fill: ui.style().visuals.panel_fill,
                    inner_margin: DesignTokens::panel_margin(),
                    ..Default::default()
                }
                .show(ui, |ui| {
                    let mut scene_manager = SceneManager::get_mut();
                    let scene = scene_manager.simulation_scene_mut();
                    re_ui::list_item::list_item_scope(ui, "scene", |ui| {
                        let mut list_item = ui.list_item().draggable(false);
                        if self.target_container == Some(scene.root()) {
                            list_item = list_item
                                .force_background(ui.style().visuals.widgets.hovered.weak_bg_fill);
                        }
                        let response = list_item.show_flat(
                            ui,
                            re_ui::list_item::LabelContent::new("Scene")
                                .truncate(true)
                                .always_show_buttons(true)
                                .with_icon(&icons::OBJECT_TREE)
                                .with_buttons(|ui| {
                                    self.add_game_object_button(ui, scene, &mut app_state)
                                }),
                        );
                        for root_object in scene.root_objects().collect::<Vec<_>>() {
                            self.render_scene_node(scene, &mut app_state, ui, root_object, true);
                        }
                        self.handle_root_dnd_interaction(ui, scene, &response);

                        let empty_space_response =
                            ui.allocate_response(ui.available_size(), egui::Sense::click());

                        if empty_space_response.clicked() {
                            app_state.selection = Selection::none();
                        }

                        self.handle_empty_space_dnd_interaction(
                            ui,
                            scene,
                            empty_space_response.rect,
                        );
                    });
                });
            });

        self.target_container = None;
        while let Ok(command) = self.receiver.try_recv() {
            match command {
                Command::SetTargetContainer(game_object) => {
                    self.target_container = Some(game_object)
                }
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl PanelSceneHierarchy {
    fn render_scene_node(
        &mut self,
        scene: &mut Scene,
        app_state: &mut EditorAppState,
        ui: &mut Ui,
        game_object: GameObject,
        parent_visible: bool,
    ) {
        let game_object_id = scene.get_game_object_uuid(game_object);
        let id = ui.make_persistent_id(game_object_id);
        let name = scene.get_game_object_name(game_object);
        let children = scene.get_children_ordered(game_object).collect::<Vec<_>>();
        let is_selected = app_state
            .selection
            .contains(SelectionType::GameObject, game_object_id);
        let mut visible = scene
            .read_component::<ComponentID, _, _>(game_object, |c| c.visible)
            .unwrap_or(false);
        let container_visible = visible && parent_visible;
        let mut item = re_ui::list_item::ListItem::new()
            .selected(is_selected)
            .draggable(true);
        if self.target_container == Some(game_object) {
            item = item.force_background(ui.style().visuals.widgets.hovered.weak_bg_fill);
        }
        let response;
        let body_response;
        let mut visibility_response = None;
        let content = re_ui::list_item::LabelContent::new(name)
            .truncate(true)
            .subdued(!container_visible)
            .with_icon(&icons::GAME_OBJECT)
            .with_buttons(|ui| {
                let res = Self::visibility_button_ui(ui, parent_visible, &mut visible);
                visibility_response = Some(res.clone());
                res
            });
        if !children.is_empty() {
            let res = item.show_hierarchical_with_children(ui, id, true, content, |ui| {
                for child_node in children {
                    self.render_scene_node(scene, app_state, ui, child_node, container_visible)
                }
            });
            response = res.item_response;
            body_response = res.body_response.map(|r| r.response);
        } else {
            response = item.show_hierarchical(ui, content);
            body_response = None;
        }
        self.handle_interaction(
            ui,
            scene,
            id,
            game_object,
            is_selected,
            visible,
            app_state,
            &response,
            body_response.as_ref(),
            visibility_response.as_ref(),
        );
    }

    fn handle_interaction(
        &mut self,
        ui: &mut Ui,
        scene: &mut Scene,
        id: egui::Id,
        game_object: GameObject,
        is_selected: bool,
        is_visible: bool,
        app_state: &mut EditorAppState,
        response: &Response,
        body_response: Option<&Response>,
        visibility_response: Option<&Response>,
    ) {
        if response.double_clicked() {
            app_state.selection = Selection::from_id(
                SelectionType::GameObject,
                scene.get_game_object_uuid(game_object),
            );
            if let Some(state) = egui::collapsing_header::CollapsingState::load(ui.ctx(), id) {
                state.store(ui.ctx());
            }
        } else if response.clicked() {
            app_state.selection = if is_selected {
                Selection::none()
            } else {
                Selection::from_id(
                    SelectionType::GameObject,
                    scene.get_game_object_uuid(game_object),
                )
            };
        } else if response.secondary_clicked() {
            app_state.selection = Selection::from_id(
                SelectionType::GameObject,
                scene.get_game_object_uuid(game_object),
            );
        }
        if visibility_response.map(|r| r.changed()).unwrap_or(false) {
            scene.write_component::<ComponentID, _>(game_object, |c| {
                c.visible = is_visible;
            });
        }
        self.handle_dnd_interaction(ui, scene, game_object, response, body_response);
        response.context_menu(|ui| {
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
                }
                ui.close_menu();
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

    fn handle_root_dnd_interaction(&mut self, ui: &mut Ui, scene: &mut Scene, response: &Response) {
        let Some(dragged_game_object) = self.dragged_game_object(ui, scene) else {
            return;
        };

        let item_desc = re_ui::drag_and_drop::ItemContext {
            id: scene.get_game_object_uuid(scene.root()),
            item_kind: ItemKind::RootContainer,
            previous_container_id: None,
        };

        let drop_target = re_ui::drag_and_drop::find_drop_target(
            ui,
            &item_desc,
            response.rect,
            None,
            DesignTokens::list_item_height(),
        );

        if let Some(drop_target) = drop_target {
            self.handle_drop_target(ui, scene, drop_target, dragged_game_object);
        }
    }

    fn handle_empty_space_dnd_interaction(
        &mut self,
        ui: &mut Ui,
        scene: &mut Scene,
        empty_space: egui::Rect,
    ) {
        let Some(dragged_game_object) = self.dragged_game_object(ui, scene) else {
            return;
        };

        if ui.rect_contains_pointer(empty_space) {
            let drop_target = DropTarget::new(
                empty_space.x_range(),
                empty_space.top(),
                scene.root_id(),
                usize::MAX,
            );

            self.handle_drop_target(ui, scene, drop_target, dragged_game_object);
        }
    }

    fn handle_dnd_interaction(
        &mut self,
        ui: &mut Ui,
        scene: &mut Scene,
        game_object: GameObject,
        response: &Response,
        body_response: Option<&Response>,
    ) {
        if response.drag_started() {
            egui::DragAndDrop::set_payload(ui.ctx(), scene.get_game_object_uuid(game_object));
        }
        let Some(dragged_game_object) = self.dragged_game_object(ui, scene) else {
            return;
        };

        let game_object_id = scene.get_game_object_uuid(game_object);
        let parent = scene.get_parent_game_object(game_object).unwrap();
        let parent_id = scene.get_game_object_uuid(parent);
        let position_index_in_parent = scene
            .get_index_in_parent(parent, game_object, SiblingDir::Before)
            .unwrap() as usize;

        let previous_container_id = if position_index_in_parent > 0 {
            scene
                .get_child_by_index(parent, (position_index_in_parent - 1) as i32)
                .map(|go| scene.get_game_object_uuid(go))
        } else {
            None
        };

        let item_desc = re_ui::drag_and_drop::ItemContext {
            id: game_object_id,
            item_kind: ItemKind::Container {
                parent_id,
                position_index_in_parent,
            },
            previous_container_id,
        };

        let drop_target = re_ui::drag_and_drop::find_drop_target(
            ui,
            &item_desc,
            response.rect,
            body_response.map(|r| r.rect),
            DesignTokens::list_item_height(),
        );

        if let Some(drop_target) = drop_target {
            self.handle_drop_target(ui, scene, drop_target, dragged_game_object);
        }
    }

    fn handle_drop_target(
        &mut self,
        ui: &mut Ui,
        scene: &mut Scene,
        drop_target: DropTarget<Uuid>,
        dragged_game_object: GameObject,
    ) {
        let Some(target_parent) = scene.get_game_object_by_uuid(drop_target.target_parent_id)
        else {
            return;
        };

        if scene.is_descendant(dragged_game_object, target_parent) {
            return;
        }

        let target_sibling =
            scene.get_child_by_index(target_parent, drop_target.target_position_index as i32);

        ui.painter().hline(
            drop_target.indicator_span_x,
            drop_target.indicator_position_y,
            (2.0, Color32::WHITE),
        );

        if ui.input(|i| i.pointer.any_released()) {
            scene.set_parent_with_sibling(
                dragged_game_object,
                Some(target_parent),
                target_sibling.map(|sibling| (sibling, SiblingDir::Before)),
            );
            egui::DragAndDrop::clear_payload(ui.ctx());
        } else {
            self.send_command(Command::SetTargetContainer(target_parent));
        }
    }

    fn dragged_game_object(&mut self, ui: &Ui, scene: &Scene) -> Option<GameObject> {
        let dragged_game_object_id =
            egui::DragAndDrop::payload::<Uuid>(ui.ctx()).map(|payload| (*payload))?;
        if let Some(go) = scene.get_game_object_by_uuid(dragged_game_object_id) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            Some(go)
        } else {
            None
        }
    }

    fn add_game_object_button(
        &self,
        ui: &mut Ui,
        scene: &mut Scene,
        app_state: &EditorAppState,
    ) -> Response {
        let res = ui
            .small_icon_button(&re_ui::icons::ADD)
            .on_hover_text("Add a new game object");
        if res.clicked() {
            let parent = app_state
                .selection
                .last(SelectionType::GameObject)
                .and_then(|id| scene.get_game_object_by_uuid(id));
            scene.create_game_object(None, parent);
        }
        res
    }

    fn visibility_button_ui(ui: &mut Ui, enabled: bool, visible: &mut bool) -> Response {
        ui.add_enabled_ui(enabled, |ui| {
            ui.visibility_toggle_button(visible)
                .on_hover_text("Toggle visibility")
                .on_disabled_hover_text("A parent is invisible")
        })
        .inner
    }

    fn send_command(&self, cmd: Command) {
        self.sender.send(cmd).ok();
    }
}
