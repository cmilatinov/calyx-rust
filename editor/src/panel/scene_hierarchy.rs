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
        // TODO: Need better API for nodes
        let mut app_state = EditorAppState::get_mut();
        let entities = app_state.scene.root_entities().clone();
        let mut selection: Option<Uuid> = app_state.selected_entity;
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
        if (selection.is_some()) {
            println!("{:?} {:?}", selection, app_state.selected_entity);
        }
    }
}

impl PanelSceneHierarchy {
    fn render_scene_node(
        &self,
        scene: &Scene,
        selected: &Option<Uuid>,
        selection: &mut Option<Uuid>,
        ui: &mut Ui,
        node_id: NodeId
    ) {
        let children: Vec<NodeId> =
            scene.get_children(node_id)
                .into_iter()
                .collect();
        let id_s = scene.world.read_storage::<ComponentID>();
        let id = id_s.get(scene.get_entity(node_id).unwrap()).unwrap();

        if children.len() > 0 {
            for child_node in children {
                ui.collapsing(id.name.as_str(), |ui| {
                    self.render_scene_node(scene, selected, selection, ui, child_node);
                });
            }
        } else {
            let selected = if let Some(selected_id) = selected {
                *selected_id == id.id
            } else { false };
            let res = ui.selectable_label(
                selected,
                id.name.as_str()
            );
            if res.clicked() {
                *selection =
                    if selected { None }
                    else { Some(id.id) };
            }
        }
    }
}
