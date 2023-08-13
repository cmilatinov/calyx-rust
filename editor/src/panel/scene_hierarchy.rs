use egui::Ui;
use specs::WorldExt;

use engine::*;
use engine::ecs::ComponentID;
use engine::indextree::NodeId;
use engine::scene::Scene;
use engine::uuid::Uuid;
use crate::EditorAppState;

use crate::panel::Panel;

pub struct PanelSceneHierarchy;

impl Default for PanelSceneHierarchy {
    fn default() -> Self {
        PanelSceneHierarchy {}
    }
}

impl Panel for PanelSceneHierarchy {
    fn name() -> &'static str {
        "Scene Hierarchy"
    }

    fn ui(&self, ui: &mut Ui) {
        let mut app_state = EditorAppState::get_mut();
        let entities = app_state.scene.root_entities().clone();
        let mut selection: Option<NodeId> = app_state.selected_entity;
        for root_node in entities {
            self.render_scene_node(
                &app_state.scene,
                &app_state.selected_entity,
                &mut selection,
                ui,
                root_node
            );
        }
        app_state.selected_entity = selection;
    }
}

impl PanelSceneHierarchy {
    fn render_scene_node(
        &self,
        scene: &Scene,
        selected: &Option<NodeId>,
        selection: &mut Option<NodeId>,
        ui: &mut Ui,
        node_id: NodeId
    ) {
        let children: Vec<NodeId> =
            scene.get_children(node_id)
                .into_iter()
                .collect();

        if children.len() > 0 {
            for child_node in children {
                let collapsing_id = ui.make_persistent_id("my_collapsing_header");
                egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), collapsing_id, false)
                    .show_header(ui, |ui| {
                        let selected = if let Some(selected_id) = selected {
                            *selected_id == node_id
                        } else { false };
                        let res = ui.selectable_label(
                            selected,
                            scene.get_entity_name(node_id).expect("Entity with no name")
                        );
                        if res.clicked() {
                            *selection =
                                if selected { None }
                                else { Some(node_id) };
                        }
                    })
                    .body(|ui| self.render_scene_node(scene, selected, selection, ui, child_node));
            }
        } else {
            let selected = if let Some(selected_id) = selected {
                *selected_id == node_id
            } else { false };
            let res = ui.selectable_label(
                selected,
                scene.get_entity_name(node_id).expect("Entity with no name")
            );
            if res.clicked() {
                *selection =
                    if selected { None }
                    else { Some(node_id) };
            }
        }
    }
}
