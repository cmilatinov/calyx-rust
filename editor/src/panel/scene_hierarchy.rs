use std::collections::HashSet;
use egui::Ui;

use engine::*;
use engine::assets::AssetRegistry;
use engine::assets::mesh::Mesh;
use engine::component::ComponentMesh;
use engine::indextree::NodeId;
use engine::scene::Scene;
use crate::{EditorAppState, EditorSelection};
use crate::panel::Panel;

#[derive(Default)]
pub struct PanelSceneHierarchy {
    search: String
}

impl Panel for PanelSceneHierarchy {
    fn name() -> &'static str {
        "Scene Hierarchy"
    }

    fn ui(&mut self, ui: &mut Ui) {
        let mut app_state = EditorAppState::get_mut();
        let entities = app_state.scene.root_entities().clone();
        let mut selection = app_state.selection.clone();

        ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
            if ui.button("+").clicked() {
                let mesh = AssetRegistry::get_mut().load::<Mesh>("meshes/cube").unwrap();
                let mut parent: Option<NodeId> = None;
                if let Some(selected) = selection.clone() {
                    match selected {
                        EditorSelection::Entity(set) => {
                            for x in set.iter() {
                                parent = Some(*x);
                            }
                        }
                        EditorSelection::Asset(_) => {}
                    }
                }

                let new_entity = app_state.scene.create_entity(None, parent);
                app_state.scene.bind_component(new_entity, ComponentMesh { mesh: mesh.clone() }).unwrap();
            }
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search Here"));
        });

        for root_node in entities {
            self.render_scene_node(
                &app_state.scene,
                &app_state.selection,
                &mut selection,
                ui,
                root_node
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
        node_id: NodeId
    ) {
        let children: Vec<NodeId> =
            scene.get_children(node_id)
                .into_iter()
                .collect();

        let is_selected = if let Some(editor_selection) = selected {
            if let EditorSelection::Entity(set) = editor_selection {
                set.contains(&node_id)
            } else {
                false
            }
        } else { false };

        if children.len() > 0 {
            let collapsing_id = ui.make_persistent_id(node_id);
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), collapsing_id, false)
                .show_header(ui, |ui| {
                    self.show_selectable_label(scene, is_selected, selection, ui, node_id);
                })
                .body(|ui| {
                    for child_node in children {
                        self.render_scene_node(scene, selected, selection, ui, child_node)
                    }
                });
        } else {
            self.show_selectable_label(scene, is_selected, selection, ui, node_id);
        }
    }

    fn show_selectable_label(&self,
                             scene: &Scene,
                             is_selected: bool,
                             selection: &mut Option<EditorSelection>,
                             ui: &mut Ui,
                             node_id: NodeId) {
        let res = ui.selectable_label(
            is_selected,
            scene.get_entity_name(node_id).expect("Entity with no name")
        );

        if res.clicked() {
            let mut set = HashSet::new();
            set.insert(node_id);
            *selection =
                if is_selected { None }
                else { Some(EditorSelection::Entity(set)) };
        }
    }
}
