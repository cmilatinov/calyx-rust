use std::any::Any;
use std::collections::{BTreeSet, HashSet};

use crate::inspector::assets::animation_graph_inspector::AnimationGraphInspector;
use crate::inspector::inspector_registry::InspectorRegistry;
use crate::inspector::type_inspector::InspectorContext;
use crate::inspector::widgets::{SearchSelectState, Widgets};
use crate::panel::Panel;
use crate::selection::SelectionType;
use crate::EditorAppState;
use convert_case::{Case, Casing};
use egui::scroll_area::ScrollBarVisibility;
use egui::{Id, PopupCloseBehavior, Ui};
use engine::assets::animation_graph::AnimationGraph;
use engine::component::{ComponentID, ComponentTransform};
use engine::context::ReadOnlyRegistryContext;
use engine::reflect::type_registry::TypeRegistry;
use engine::reflect::{AttributeValue, NamedField, Reflect, StructInfo, TypeInfo};
use engine::scene::GameObject;
use engine::utils::TypeUuid;
use re_ui::list_item::{LabelContent, ListItem, PropertyContent};
use re_ui::{DesignTokens, UiExt};
use serde_json::Value;
use uuid::Uuid;

#[derive(Default)]
pub struct PanelInspector {
    add_component_select: SearchSelectState,
}

impl Panel for PanelInspector {
    fn name() -> &'static str {
        "Inspector"
    }

    fn ui(&mut self, ui: &mut Ui, state: &mut EditorAppState) {
        let type_registry_ref = state.game.assets.registries.types.clone();
        let type_registry = type_registry_ref.read();

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
                        if let Some(game_object) = state
                            .selection
                            .first(SelectionType::GameObject)
                            .and_then(|id| state.game.scenes.simulation_scene().find(id))
                        {
                            let game_object_id = state.game.scenes.simulation_scene().uuid(game_object);
                            let mut structural_changed = false;
                            let mut structural_edit_before = None;
                            let mut entity_components = HashSet::new();
                            let mut components_to_remove = HashSet::new();
                            let component_registry_ref =
                                state.game.assets.registries.components.clone();
                            let component_registry = component_registry_ref.read();
                            if let Some(entry) =
                                state.game.scenes.simulation_scene().entry(game_object)
                            {
                                for (type_id, component) in component_registry.components() {
                                    if component.get_instance(&entry).is_some() {
                                        entity_components.insert(*type_id);
                                    }
                                }
                            }

                            let component_to_add = self.add_component_button_ui(
                                ui,
                                &state.game.assets.lock_read().registries,
                                &entity_components,
                            );
                            if let Some((type_uuid, name)) = component_to_add {
                                Self::capture_structural_scene_edit(
                                    &mut structural_edit_before,
                                    state,
                                );
                                let scene = state.game.scenes.simulation_scene_mut();
                                scene.bind_component_dyn(game_object, type_uuid);
                                entity_components.insert(type_uuid);
                                structural_changed = true;
                                log::info!(
                                    "Added component to game object: component={} type_uuid={} object={}",
                                    name,
                                    type_uuid,
                                    Self::game_object_label(scene, game_object)
                                );
                            }

                            for (type_id, component) in component_registry.components() {
                                entity_components.insert(*type_id);
                                let Some(TypeInfo::Struct(type_info)) =
                                    type_registry.type_info_by_id(*type_id)
                                else {
                                    continue;
                                };
                                let Some(instance) = (unsafe {
                                    state
                                        .game
                                        .scenes
                                        .simulation_scene_mut()
                                        .get_component_ptr(game_object, &**component)
                                }) else {
                                    continue;
                                };
                                let simulation_scene = state.game.scenes.simulation_scene();
                                let ctx = InspectorContext {
                                    assets: &state.game.assets.lock_read().registries,
                                    scene: simulation_scene,
                                    game_object,
                                    parent: simulation_scene.parent(game_object),
                                    type_info,
                                    field_name: None,
                                };
                                let (before, after, remove) = {
                                    let instance = unsafe { &mut *instance };
                                    let before = instance.serialize();
                                    let remove = self.show_inspector(
                                        ui,
                                        &state.inspector_registry,
                                        &ctx,
                                        instance.as_reflect_mut(),
                                    );
                                    let after = instance.serialize();
                                    (before, after, remove)
                                };
                                if remove {
                                    components_to_remove.insert(*type_id);
                                }
                                if let (Some(before), Some(after)) = (before, after) {
                                    let changes = Self::changed_json_values(&before, &after);
                                    state.record_inspector_value_edits(
                                        game_object_id,
                                        *type_id,
                                        &Self::type_display_name(&type_registry, *type_id)
                                            .unwrap_or_else(|| type_id.to_string()),
                                        changes,
                                    );
                                }
                            }
                            for (type_id, component) in component_registry.components() {
                                if !components_to_remove.contains(type_id) {
                                    continue;
                                }
                                let component_name =
                                    Self::type_display_name(&type_registry, *type_id)
                                        .unwrap_or_else(|| type_id.to_string());
                                let object_label = Self::game_object_label(
                                    state.game.scenes.simulation_scene(),
                                    game_object,
                                );
                                Self::capture_structural_scene_edit(
                                    &mut structural_edit_before,
                                    state,
                                );
                                if let Some(mut entry) = state
                                    .game
                                    .scenes
                                    .simulation_scene_mut()
                                    .entry_mut(game_object)
                                {
                                    component.remove_instance(&mut entry);
                                    structural_changed = true;
                                    log::info!(
                                        "Removed component from game object: component={} type_uuid={} object={}",
                                        component_name,
                                        type_id,
                                        object_label
                                    );
                                }
                            }
                            if structural_changed {
                                state.commit_scene_edit(
                                    "Edit inspector components",
                                    structural_edit_before,
                                );
                            }
                        } else if let Some(asset_id) = state.selection.first(SelectionType::Asset) {
                            let asset_registry_ref = state.game.assets.registries.assets.clone();
                            let asset_registry = asset_registry_ref.read();
                            let Some(asset_meta) = asset_registry.asset_meta_from_id(asset_id)
                            else {
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
                                .force_background(
                                    re_ui::design_tokens().section_collapsing_header_color(),
                                )
                                .show_hierarchical_with_children_unindented(
                                    ui,
                                    header_id,
                                    true,
                                    LabelContent::new(format!("{}", asset_meta.name))
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
                        } else if let SelectionType::AnimationNode(asset_id) = state.selection.ty()
                        {
                            if let Some(id) = state.selection.iter().next() {
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
                                    AnimationGraphInspector::node(
                                        ui,
                                        &asset_registry,
                                        &mut graph,
                                        id,
                                    );
                                }
                            }
                        } else if let SelectionType::AnimationTransition(asset_id) =
                            state.selection.ty()
                        {
                            if let Some(id) = state.selection.iter().next() {
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
    fn capture_structural_scene_edit(
        before: &mut Option<crate::SceneEditSnapshot>,
        state: &EditorAppState,
    ) {
        if before.is_none() {
            *before = state.scene_edit_snapshot();
        }
    }

    fn display_name(type_registry: &TypeRegistry, instance: &dyn Reflect) -> &'static str {
        type_registry
            .type_info_by_id(instance.uuid())
            .and_then(|info| {
                match info {
                    TypeInfo::Struct(info) => {
                        if let Some(AttributeValue::String(str)) = info.attr("name") {
                            return Some(str);
                        }
                    }
                    TypeInfo::Enum(info) => {
                        if let Some(AttributeValue::String(str)) = info.attr("name") {
                            return Some(str);
                        }
                    }
                    _ => {}
                }
                None
            })
            .unwrap_or(instance.type_name_short())
    }

    fn field_display_name(field: &NamedField) -> String {
        if let Some(AttributeValue::String(name)) = field.attrs.get("name") {
            (*name).into()
        } else {
            field.name.from_case(Case::Snake).to_case(Case::Title)
        }
    }

    fn show_inspector(
        &self,
        ui: &mut Ui,
        registry: &InspectorRegistry,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
    ) -> bool {
        let name = Self::display_name(&ctx.assets.types.read(), instance);
        let id = ui.make_persistent_id(name);
        let type_uuid = instance.uuid();
        let res = ListItem::new()
            .interactive(true)
            .force_background(re_ui::design_tokens().section_collapsing_header_color())
            .show_hierarchical_with_children_unindented(
                ui,
                id,
                true,
                LabelContent::new(name).truncate(true),
                |ui| {
                    if let Some(inspector) = registry.type_inspector_lookup(type_uuid) {
                        inspector.show_inspector(ui, ctx, instance);
                    } else {
                        self.show_default_inspector(ui, registry, ctx, instance);
                    }
                },
            )
            .item_response;
        if res.clicked() {
            if let Some(mut state) = egui::collapsing_header::CollapsingState::load(ui.ctx(), id) {
                state.toggle(ui);
                state.store(ui.ctx());
            }
        }
        let mut remove = false;
        if type_uuid != ComponentID::type_uuid() {
            res.context_menu(|ui| {
                if type_uuid != ComponentTransform::type_uuid() && ui.button("Remove").clicked() {
                    remove = true;
                    ui.close_menu();
                }
                if let Some(inspector) = registry.type_inspector_lookup(type_uuid) {
                    inspector.show_inspector_context(ui, ctx, instance);
                }
            });
        }
        remove
    }

    fn show_default_inspector(
        &self,
        ui: &mut Ui,
        registry: &InspectorRegistry,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
    ) {
        let type_registry = ctx.assets.types.read();
        if let Some(TypeInfo::Struct(info)) = type_registry.type_info_by_id(instance.uuid()) {
            self.show_default_struct_inspector(ui, registry, ctx, instance, info);
        }
    }

    fn show_default_struct_inspector(
        &self,
        ui: &mut Ui,
        registry: &InspectorRegistry,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
        info: &StructInfo,
    ) {
        for (_, field) in info.fields.iter() {
            let mut field_ctx = *ctx;
            field_ctx.field_name = Some(field.name);
            if let Some(value) = field.get_reflect_mut(instance.as_reflect_mut()) {
                self.show_default_inspector_field(ui, registry, &field_ctx, field, value);
            }
        }
    }

    fn show_default_inspector_field(
        &self,
        ui: &mut Ui,
        registry: &InspectorRegistry,
        ctx: &InspectorContext,
        field: &NamedField,
        instance: &mut dyn Reflect,
    ) {
        let mut name = Self::field_display_name(field);
        name.push(' ');
        let type_registry = ctx.assets.types.read();
        if let Some(inspector) = registry.type_inspector_lookup(instance.uuid()) {
            if matches!(
                type_registry.type_info_by_id(instance.uuid()),
                Some(TypeInfo::Enum(_))
            ) {
                inspector.show_inspector(ui, ctx, instance);
                return;
            }
            Widgets::inspector_prop_value(ui, name, |ui, _| {
                inspector.show_inspector(ui, ctx, instance);
            });
            return;
        }
        if let Some(TypeInfo::Struct(info)) = type_registry.type_info_by_id(instance.uuid()) {
            let mut nested_ctx = *ctx;
            nested_ctx.field_name = None;
            nested_ctx.type_info = info;
            ListItem::new()
                .interactive(false)
                .show_hierarchical_with_children(
                    ui,
                    Id::new((ctx.type_info.type_name, field.name, field.type_uuid)),
                    true,
                    PropertyContent::new(name).show_only_when_collapsed(false),
                    |ui| {
                        self.show_default_struct_inspector(
                            ui,
                            registry,
                            &nested_ctx,
                            instance,
                            info,
                        );
                    },
                );
            return;
        }

        if type_registry.type_info_by_id(instance.uuid()).is_some() {
            Widgets::inspector_prop_value(ui, name, |ui, _| {
                ui.label(instance.type_name_short());
            });
        }
    }

    fn add_component_button_ui(
        &mut self,
        ui: &mut Ui,
        assets: &ReadOnlyRegistryContext,
        entity_components: &HashSet<Uuid>,
    ) -> Option<(Uuid, String)> {
        let num_components = assets.components.read().components().count();
        let enabled = num_components > entity_components.len();
        let mut component_to_add = None;
        let res = ui
            .list_item()
            .draggable(false)
            .interactive(enabled)
            .show_flat(
                ui,
                LabelContent::new(" Add Component")
                    .always_show_buttons(true)
                    .truncate(true)
                    .with_icon(&re_ui::icons::ADD),
            )
            .on_hover_text("Add a new component to this game object");
        let id = ui.make_persistent_id("add_component_popup");
        egui::popup::popup_below_widget(
            ui,
            id,
            &res,
            PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                Widgets::search_select_contents(
                    ui,
                    id,
                    &mut self.add_component_select,
                    |ui, search| {
                        let search = search.trim().to_owned();
                        let mut shown = 0usize;
                        for (type_uuid, component) in assets.components.read().components() {
                            if entity_components.contains(type_uuid) {
                                continue;
                            }
                            let name =
                                Self::display_name(&assets.types.read(), component.as_reflect());
                            if !Self::component_matches_search(name, &search) {
                                continue;
                            }
                            shown += 1;
                            if ui.selectable_label(false, name).clicked() {
                                component_to_add = Some((*type_uuid, name.to_owned()));
                                ui.memory_mut(|mem| mem.close_popup());
                            }
                        }

                        if shown == 0 {
                            ui.weak("No matching components");
                        }
                    },
                );
            },
        );
        if res.clicked() && enabled {
            self.add_component_select.open();
            ui.memory_mut(|mem| mem.open_popup(id));
        }
        if component_to_add.is_some() {
            self.add_component_select.clear_search();
        }
        component_to_add
    }

    fn component_matches_search(name: &str, search: &str) -> bool {
        let search = search.trim();
        search.is_empty() || name.to_lowercase().contains(&search.to_lowercase())
    }

    fn changed_json_values(before: &Value, after: &Value) -> Vec<(Vec<String>, Value, Value)> {
        let mut changes = Vec::new();
        Self::collect_json_value_changes(before, after, &mut Vec::new(), &mut changes);
        changes
    }

    fn collect_json_value_changes(
        before: &Value,
        after: &Value,
        path: &mut Vec<String>,
        changes: &mut Vec<(Vec<String>, Value, Value)>,
    ) {
        match (before, after) {
            (Value::Object(before), Value::Object(after))
                if before.len() == after.len()
                    && before.keys().all(|key| after.contains_key(key)) =>
            {
                let keys = before.keys().cloned().collect::<BTreeSet<_>>();
                for key in keys {
                    path.push(key.clone());
                    Self::collect_json_value_changes(&before[&key], &after[&key], path, changes);
                    path.pop();
                }
            }
            (Value::Array(before), Value::Array(after)) if before.len() == after.len() => {
                for (index, (before, after)) in before.iter().zip(after).enumerate() {
                    path.push(index.to_string());
                    Self::collect_json_value_changes(before, after, path, changes);
                    path.pop();
                }
            }
            _ if before != after => changes.push((path.clone(), before.clone(), after.clone())),
            _ => {}
        }
    }

    fn type_display_name(type_registry: &TypeRegistry, type_uuid: Uuid) -> Option<String> {
        type_registry
            .type_info_by_id(type_uuid)
            .map(|info| match info {
                TypeInfo::Struct(info) => info
                    .attr("name")
                    .and_then(|attr| match attr {
                        AttributeValue::String(name) => Some(name.to_string()),
                        _ => None,
                    })
                    .unwrap_or_else(|| Self::short_type_name(info.type_name).to_string()),
                TypeInfo::Enum(info) => info
                    .attr("name")
                    .and_then(|attr| match attr {
                        AttributeValue::String(name) => Some(name.to_string()),
                        _ => None,
                    })
                    .unwrap_or_else(|| Self::short_type_name(info.type_name).to_string()),
                TypeInfo::List(info) => Self::short_type_name(info.type_name).to_string(),
                TypeInfo::Option(info) => Self::short_type_name(info.type_name).to_string(),
                TypeInfo::Map(info) => Self::short_type_name(info.type_name).to_string(),
                TypeInfo::None => type_uuid.to_string(),
            })
    }

    fn short_type_name(type_name: &str) -> &str {
        type_name.rsplit("::").next().unwrap_or(type_name)
    }

    fn game_object_label(scene: &engine::scene::Scene, game_object: GameObject) -> String {
        format!("{} ({})", scene.name(game_object), scene.uuid(game_object))
    }
}

#[cfg(test)]
mod tests {
    use super::PanelInspector;
    use serde_json::json;

    #[test]
    fn component_search_matches_case_insensitive_substrings() {
        assert!(PanelInspector::component_matches_search(
            "Tank Controller",
            "tank"
        ));
        assert!(PanelInspector::component_matches_search(
            "Directional Light",
            "LIGHT"
        ));
        assert!(!PanelInspector::component_matches_search(
            "Component Mesh",
            "camera"
        ));
    }

    #[test]
    fn component_search_treats_blank_query_as_match() {
        assert!(PanelInspector::component_matches_search("Camera", ""));
        assert!(PanelInspector::component_matches_search("Camera", "   "));
    }

    #[test]
    fn json_value_changes_use_stable_nested_paths() {
        let before = json!({
            "transform": {
                "position": [0.0, 0.0, 0.0],
                "scale": [1.0, 1.0, 1.0]
            }
        });
        let after = json!({
            "transform": {
                "position": [2.0, 0.0, 0.0],
                "scale": [1.0, 3.0, 1.0]
            }
        });

        let changes = PanelInspector::changed_json_values(&before, &after);

        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].0, vec!["transform", "position", "0"]);
        assert_eq!(changes[0].1, json!(0.0));
        assert_eq!(changes[0].2, json!(2.0));
        assert_eq!(changes[1].0, vec!["transform", "scale", "1"]);
    }
}
