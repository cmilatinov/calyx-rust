use egui::Ui;

use engine::*;
use engine::indextree::NodeId;
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

    fn ui(&mut self, ui: &mut Ui) {
        {
            // TODO: Need better API for nodes
            // let mut app_state = EditorAppState::get();
            //
            // for root_node in app_state.scene.root_entities() {
            //     let mut stack: Vec<&NodeId> = vec![root_node];
            //
            //     while let Some(node_id) = stack.pop() {
            //         if app_state.scene.get_children_count(*node_id) > 0 {
            //             ui.collapsing()
            //         }
            //     }
            // }
        }
    }
}
