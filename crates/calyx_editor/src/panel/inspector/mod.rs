use std::any::Any;
use std::collections::HashSet;

use crate::inspector::assets::animation_graph_inspector::AnimationGraphInspector;
use crate::inspector::component_inspector::{type_display_name, ComponentInspector};
use crate::inspector::type_inspector::InspectorContext;
use crate::panel::Panel;
use crate::scene_edit::SceneEdit;
use crate::selection::SelectionType;
use crate::EditorAppState;
use calyx_engine::assets::animation_graph::AnimationGraph;
use calyx_engine::reflect::TypeInfo;
use calyx_engine::scene::GameObject;
use egui::scroll_area::ScrollBarVisibility;
use egui::{Id, Ui};
use re_ui::list_item::{LabelContent, ListItem};
use re_ui::{DesignTokens, UiExt};
use uuid::Uuid;

use self::add_component::AddComponentButton;

mod add_component;
mod structural_edit;

#[derive(Default)]
pub struct PanelInspector {
    add_component: AddComponentButton,
}

impl Panel for PanelInspector {
    fn name() -> &'static str {
        "Inspector"
    }

    fn ui(&mut self, ui: &mut Ui, state: &mut EditorAppState) {
        state.inspector_rect = Some(ui.clip_rect());
        egui::ScrollArea::both()
            .auto_shrink([true, true])
            .scroll_bar_visibility(ScrollBarVisibility::VisibleWhenNeeded)
            .show(ui, |ui| {
                egui::Frame {
                    fill: ui.style().visuals.panel_fill,
                    inner_margin: DesignTokens::panel_margin(),
                    ..Default::default()
                }
                .show(ui, |ui| {
                    re_ui::list_item::list_item_scope(ui, "inspector_scope", |ui| {
                        self.selection_ui(ui, state);
                    });
                    ui.allocate_space(ui.available_size());
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

impl PanelInspector {
    /// Draws whatever is selected: a game object takes precedence over an
    /// asset, then an animation graph node or transition.
    fn selection_ui(&mut self, ui: &mut Ui, state: &mut EditorAppState) {
        let selected_game_object = state
            .selection
            .first(SelectionType::GameObject)
            .and_then(|id| state.game.scenes.simulation_scene().find(id));
        state.select_inspector_object(
            selected_game_object.map(|go| state.game.scenes.simulation_scene().uuid(go)),
        );
        if let Some(game_object) = selected_game_object {
            self.game_object_ui(ui, state, game_object);
        } else if let Some(asset_id) = state.selection.first(SelectionType::Asset) {
            Self::asset_ui(ui, state, asset_id);
        } else if let SelectionType::AnimationNode(asset_id) = state.selection.ty() {
            Self::animation_node_ui(ui, state, asset_id);
        } else if let SelectionType::AnimationTransition(asset_id) = state.selection.ty() {
            Self::animation_transition_ui(ui, state, asset_id);
        }
    }

    /// Draws `game_object`'s components and the button that adds more. The
    /// components added and removed this frame are one undo step; value
    /// edits are tracked per component by the scene document.
    fn game_object_ui(&mut self, ui: &mut Ui, state: &mut EditorAppState, game_object: GameObject) {
        let mut structure = SceneEdit::begin(structural_edit::LABEL);
        let entity_components = Self::component_type_uuids(state, game_object);
        let component_to_add = self.add_component.ui(
            ui,
            &state.game.assets.lock_read().registries,
            &entity_components,
        );
        if let Some(type_uuid) = component_to_add {
            structural_edit::add(&mut structure, &mut state.game, game_object, type_uuid);
        }
        for type_uuid in Self::components_ui(ui, state, game_object) {
            structural_edit::remove(&mut structure, &mut state.game, game_object, type_uuid);
        }
        state.commit_scene_edit(structure);
    }

    /// The type UUIDs of the registered components `game_object` has.
    fn component_type_uuids(state: &EditorAppState, game_object: GameObject) -> HashSet<Uuid> {
        let component_types = state
            .game
            .scenes
            .simulation_scene()
            .component_types(game_object);
        state
            .game
            .assets
            .registries
            .components
            .read()
            .components()
            .filter(|(_, component)| component_types.contains(&component.component_type_id()))
            .map(|(type_uuid, _)| *type_uuid)
            .collect()
    }

    /// Draws each registered component `game_object` has through the scene
    /// document's edit tracker, pinned components first, then the rest by
    /// name. Applies the pin edit asked for from a header's menu. Returns the
    /// components whose removal was asked for, in display order.
    fn components_ui(
        ui: &mut Ui,
        state: &mut EditorAppState,
        game_object: GameObject,
    ) -> Vec<Uuid> {
        // Play mode records no undo history, so the inspector keeps no
        // baselines there.
        let track_edits = state.scene_document.track_inspector_edits(&state.game);
        let pointer_down = ui.input(|input| input.pointer.any_down());
        // Read before the component registry is locked below, which this
        // reads too.
        let component_types = Self::component_type_uuids(state, game_object);
        let type_registry_ref = state.game.assets.registries.types.clone();
        let type_registry = type_registry_ref.read();
        let component_registry_ref = state.game.assets.registries.components.clone();
        let component_registry = component_registry_ref.read();
        let inspector = ComponentInspector::new(&state.inspector_registry);
        let order = state.pinned_components.order(
            component_types
                .into_iter()
                .filter_map(|uuid| Some((uuid, type_display_name(&type_registry, uuid)?))),
        );
        let pin_states = state.pinned_components.states(&order);
        let mut removals = Vec::new();
        let mut pin_edit = None;
        for (type_uuid, pin) in order.iter().zip(pin_states) {
            let Some(component) = component_registry.component(*type_uuid) else {
                continue;
            };
            let Some(TypeInfo::Struct(type_info)) = type_registry.type_info_by_id(*type_uuid)
            else {
                continue;
            };
            let assets = state.game.assets.lock_read();
            let Some(shown) = state.scene_document.inspector_edits_mut().show_component(
                state.game.scenes.simulation_scene_mut(),
                game_object,
                component,
                track_edits,
                pointer_down,
                |instance, scene| {
                    let ctx = InspectorContext {
                        assets: &assets.registries,
                        scene,
                        game_object,
                        parent: scene.parent(game_object),
                        type_info,
                        field_name: None,
                    };
                    let response = inspector.show(ui, &ctx, instance.as_reflect_mut(), pin);
                    (response.edit, (response.remove, response.pin))
                },
            ) else {
                continue;
            };
            drop(assets);
            let (remove, pin) = shown.value;
            if remove {
                removals.push(*type_uuid);
            }
            if let Some(edit) = pin {
                pin_edit = Some((*type_uuid, edit));
            }
            state.scene_document.inspector_shown(
                &state.game,
                shown.changed,
                shown.completed,
                shown.committed,
            );
        }
        if let Some((type_uuid, edit)) = pin_edit {
            state
                .pinned_components
                .edit(&type_registry, type_uuid, edit, &order);
        }
        removals
    }

    fn asset_ui(ui: &mut Ui, state: &mut EditorAppState, asset_id: Uuid) {
        let asset_registry_ref = state.game.assets.registries.assets.clone();
        let asset_registry = asset_registry_ref.read();
        let Some(asset_meta) = asset_registry.asset_meta_from_id(asset_id) else {
            return;
        };
        let Some(inspector) = state
            .inspector_registry
            .asset_inspector_lookup(asset_meta.type_uuid)
        else {
            return;
        };

        let header_id = Id::new(asset_id);
        ListItem::new()
            .interactive(true)
            .force_background(re_ui::design_tokens().section_collapsing_header_color())
            .show_hierarchical_with_children_unindented(
                ui,
                header_id,
                true,
                LabelContent::new(asset_meta.name.to_string())
                    .truncate(true)
                    .always_show_buttons(true)
                    .with_buttons(|ui| {
                        let popup_id = header_id.with("popup");
                        let res = ui.small_icon_button(&re_ui::icons::MORE);
                        if res.clicked() {
                            ui.memory_mut(|mem| mem.open_popup(popup_id))
                        }
                        ui.list_item_popup(popup_id, &res, 0.0, |ui| {
                            if ui
                                .list_item()
                                .show_flat(ui, LabelContent::new("Save"))
                                .clicked()
                            {
                                asset_registry.persist(asset_id);
                            }
                        });
                        res
                    }),
                |ui| {
                    inspector.show_inspector(ui, &mut state.game, asset_id);
                },
            );
    }

    fn animation_node_ui(ui: &mut Ui, state: &mut EditorAppState, asset_id: Uuid) {
        let Some(id) = state.selection.iter().next() else {
            return;
        };
        if let Ok(graph_ref) = state
            .game
            .assets
            .registries
            .assets
            .read()
            .load_by_id::<AnimationGraph>(asset_id)
        {
            let mut graph = graph_ref.write();
            let asset_registry = state.game.assets.registries.assets.read();
            AnimationGraphInspector::node(ui, &asset_registry, &mut graph, id);
        }
    }

    fn animation_transition_ui(ui: &mut Ui, state: &mut EditorAppState, asset_id: Uuid) {
        let Some(id) = state.selection.iter().next() else {
            return;
        };
        if let Ok(graph_ref) = state
            .game
            .assets
            .registries
            .assets
            .read()
            .load_by_id::<AnimationGraph>(asset_id)
        {
            let mut graph = graph_ref.write();
            AnimationGraphInspector::transition(ui, &mut graph, id);
        }
    }
}
