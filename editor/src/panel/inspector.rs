use std::any::Any;
use std::cell::Cell;
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
use engine::reflect::{
    AttributeValue, ListInfo, NamedField, Reflect, ReflectDefault, ReflectList, StructInfo,
    TypeInfo,
};
use engine::scene::GameObject;
use engine::utils::TypeUuid;
use re_ui::list_item::{LabelContent, ListItem, PropertyContent};
use re_ui::{DesignTokens, UiExt};
use serde_json::Value;
use uuid::Uuid;

/// A structural change to a reflected list made from the inspector.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ListEdit {
    /// Appends a default element.
    Add,
    /// Removes the element at this index.
    Remove(usize),
}

/// The element type's [`ReflectDefault`], which the inspector needs to add
/// elements. A list with no registered [`ListInfo`] cannot grow.
fn list_element_default<'a>(
    type_registry: &'a TypeRegistry,
    info: Option<&ListInfo>,
) -> Option<&'a ReflectDefault> {
    type_registry.trait_meta::<ReflectDefault>(info?.element.type_uuid)
}

/// Applies `edit` to `list`, returning whether the list changed.
fn apply_list_edit(
    type_registry: &TypeRegistry,
    info: Option<&ListInfo>,
    list: &mut dyn ReflectList,
    edit: ListEdit,
) -> bool {
    match edit {
        ListEdit::Add => list_element_default(type_registry, info)
            .is_some_and(|default| list.push(default.default()).is_ok()),
        ListEdit::Remove(index) => list.remove(index).is_some(),
    }
}

/// How the inspector draws one list element.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ElementView {
    /// A type inspector drawn inline on the element's row.
    Inline,
    /// An enum type inspector, which draws its own rows under the element.
    Enum,
    /// A struct's fields, under the element.
    Struct,
    /// A nested list's elements, under the element.
    List,
    /// The element's type name, read-only.
    Label,
}

/// Picks how to draw a list element from its type inspector, its registered
/// type info and whether it is itself a list.
fn element_view(has_inspector: bool, info: Option<&TypeInfo>, is_list: bool) -> ElementView {
    match info {
        Some(TypeInfo::Enum(_)) if has_inspector => ElementView::Enum,
        _ if has_inspector => ElementView::Inline,
        Some(TypeInfo::Struct(_)) => ElementView::Struct,
        _ if is_list => ElementView::List,
        _ => ElementView::Label,
    }
}

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
                                        "Removed component from game object: component={component_name} type_uuid={type_id} object={object_label}"
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
        let id = Id::new((ctx.type_info.type_name, field.name, field.type_uuid));
        match type_registry.type_info_by_id(instance.uuid()) {
            Some(TypeInfo::Struct(info)) => {
                let mut nested_ctx = *ctx;
                nested_ctx.field_name = None;
                nested_ctx.type_info = info;
                ListItem::new()
                    .interactive(false)
                    .show_hierarchical_with_children(
                        ui,
                        id,
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
            }
            Some(TypeInfo::List(info)) => {
                self.show_list_inspector(
                    ui,
                    registry,
                    ctx,
                    &type_registry,
                    name,
                    id,
                    instance,
                    Some(info),
                );
            }
            Some(_) => {
                Widgets::inspector_prop_value(ui, name, |ui, _| {
                    ui.label(instance.type_name_short());
                });
            }
            None => {}
        }
    }

    /// Shows a list's elements, each removable, with a button that appends a
    /// default element when the element type has one.
    #[allow(clippy::too_many_arguments)]
    fn show_list_inspector(
        &self,
        ui: &mut Ui,
        registry: &InspectorRegistry,
        ctx: &InspectorContext,
        type_registry: &TypeRegistry,
        name: String,
        id: Id,
        instance: &mut dyn Reflect,
        info: Option<&ListInfo>,
    ) {
        let Some(list) = instance.as_list_mut() else {
            return;
        };
        let can_add = list_element_default(type_registry, info).is_some();
        let edit = Cell::new(None);
        let len = list.len();
        let summary = match len {
            1 => String::from("1 element"),
            len => format!("{len} elements"),
        };
        ListItem::new()
            .interactive(false)
            .show_hierarchical_with_children(
                ui,
                id,
                true,
                PropertyContent::new(name)
                    .value_text(summary)
                    .action_button_with_enabled(&re_ui::icons::ADD, can_add, || {
                        edit.set(Some(ListEdit::Add));
                    }),
                |ui| {
                    for index in 0..len {
                        let Some(element) = list.element_mut(index) else {
                            continue;
                        };
                        ui.push_id(index, |ui| {
                            if self.show_list_element(
                                ui,
                                registry,
                                ctx,
                                type_registry,
                                id.with(index),
                                index,
                                element,
                            ) {
                                edit.set(Some(ListEdit::Remove(index)));
                            }
                        });
                    }
                },
            );
        if let Some(edit) = edit.get() {
            apply_list_edit(type_registry, info, list, edit);
        }
    }

    /// Shows one list element, returning `true` when its remove button was clicked.
    #[allow(clippy::too_many_arguments)]
    fn show_list_element(
        &self,
        ui: &mut Ui,
        registry: &InspectorRegistry,
        ctx: &InspectorContext,
        type_registry: &TypeRegistry,
        id: Id,
        index: usize,
        element: &mut dyn Reflect,
    ) -> bool {
        let mut remove = false;
        let label = format!("Element {index} ");
        let element_info = type_registry.type_info_by_id(element.uuid());
        let inspector = registry.type_inspector_lookup(element.uuid());
        let view = element_view(
            inspector.is_some(),
            element_info,
            element.as_list().is_some(),
        );
        if let (ElementView::Inline, Some(inspector)) = (view, inspector) {
            ui.list_item_flat_noninteractive(
                PropertyContent::new(label)
                    .value_fn(|ui, _| inspector.show_inspector(ui, ctx, element))
                    .action_button(&re_ui::icons::REMOVE, || remove = true),
            );
            return remove;
        }
        ListItem::new()
            .interactive(false)
            .show_hierarchical_with_children(
                ui,
                id,
                true,
                PropertyContent::new(label)
                    .show_only_when_collapsed(false)
                    .action_button(&re_ui::icons::REMOVE, || remove = true),
                |ui| match (view, inspector, element_info) {
                    (ElementView::Enum, Some(inspector), _) => {
                        inspector.show_inspector(ui, ctx, element)
                    }
                    (ElementView::Struct, _, Some(TypeInfo::Struct(info))) => {
                        let mut nested_ctx = *ctx;
                        nested_ctx.field_name = None;
                        nested_ctx.type_info = info;
                        self.show_default_struct_inspector(
                            ui,
                            registry,
                            &nested_ctx,
                            element,
                            info,
                        );
                    }
                    (ElementView::List, _, info) => {
                        let info = match info {
                            Some(TypeInfo::List(info)) => Some(info),
                            _ => None,
                        };
                        self.show_list_inspector(
                            ui,
                            registry,
                            ctx,
                            type_registry,
                            String::from("Elements "),
                            id.with("elements"),
                            element,
                            info,
                        );
                    }
                    _ => {
                        Widgets::inspector_prop_value(ui, "Value ", |ui, _| {
                            ui.label(element.type_name_short());
                        });
                    }
                },
            );
        remove
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
        if res.clicked() && enabled {
            self.add_component_select.open();
            ui.memory_mut(|mem| mem.open_popup(id));
        }
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
    use super::{
        apply_list_edit, element_view, list_element_default, ElementView, ListEdit, PanelInspector,
    };
    use crate::scene_history::{
        InspectorValueEdit, InspectorValueKey, SceneHistory, SceneHistoryRestore,
    };
    use engine::reflect::type_registry::TypeRegistry;
    use engine::reflect::{ListInfo, Reflect, TypeInfo};
    use engine::utils::TypeUuid;
    use serde::{Deserialize, Serialize};
    use serde_json::{json, Value};
    use uuid::Uuid;

    #[derive(Default, Debug, PartialEq, TypeUuid, Reflect, Serialize, Deserialize)]
    #[uuid = "0d6f3b2a-9c4e-4a71-b58d-3e2f7a1c9d64"]
    #[repr(C)]
    struct ListHolder {
        values: Vec<f32>,
    }

    fn values_info(registry: &TypeRegistry) -> &ListInfo {
        let Some(TypeInfo::List(info)) = registry.type_info::<Vec<f32>>() else {
            panic!("expected list info for Vec<f32>");
        };
        info
    }

    /// Runs `edit` on `holder` the way the inspector panel does, then records
    /// the resulting value changes in `history`.
    fn inspector_edit(
        registry: &TypeRegistry,
        history: &mut SceneHistory,
        holder: &mut ListHolder,
        edit: impl FnOnce(&mut dyn Reflect),
    ) {
        let key = |value_path| InspectorValueKey {
            game_object: Uuid::nil(),
            component: ListHolder::type_uuid(),
            value_path,
        };
        let before = serde_json::to_value(&*holder).unwrap();
        let TypeInfo::Struct(info) = registry.type_info::<ListHolder>().unwrap() else {
            panic!("expected struct info");
        };
        let field = info.field("values").unwrap();
        edit(field.get_reflect_mut(holder.as_reflect_mut()).unwrap());
        let after = serde_json::to_value(&*holder).unwrap();
        for (path, before, after) in PanelInspector::changed_json_values(&before, &after) {
            let edit = InspectorValueEdit {
                label: "Edit List Holder".into(),
                key: key(path),
                before,
                after,
            };
            assert!(history.push_inspector_value(edit));
        }
    }

    fn restore(holder: &mut ListHolder, restore: Option<SceneHistoryRestore>) {
        let Some(SceneHistoryRestore::InspectorValue { key, value, .. }) = restore else {
            panic!("expected an inspector value restore");
        };
        let mut serialized = serde_json::to_value(&*holder).unwrap();
        assert!(crate::set_json_value(
            &mut serialized,
            &key.value_path,
            value
        ));
        *holder = serde_json::from_value(serialized).unwrap();
    }

    #[test]
    fn list_edits_round_trip_through_inspector_history() {
        let registry = TypeRegistry::new();
        let info = values_info(&registry);
        assert!(list_element_default(&registry, Some(info)).is_some());
        let mut history = SceneHistory::default();
        let mut holder = ListHolder {
            values: vec![1.0, 2.0],
        };

        inspector_edit(&registry, &mut history, &mut holder, |values| {
            let list = values.as_list_mut().unwrap();
            assert!(apply_list_edit(&registry, Some(info), list, ListEdit::Add));
        });
        assert_eq!(holder.values, vec![1.0, 2.0, 0.0]);

        inspector_edit(&registry, &mut history, &mut holder, |values| {
            let list = values.as_list_mut().unwrap();
            *list.element_mut(2).unwrap().downcast_mut::<f32>().unwrap() = 7.0;
        });
        assert_eq!(holder.values, vec![1.0, 2.0, 7.0]);

        inspector_edit(&registry, &mut history, &mut holder, |values| {
            let list = values.as_list_mut().unwrap();
            assert!(apply_list_edit(
                &registry,
                Some(info),
                list,
                ListEdit::Remove(0)
            ));
        });
        assert_eq!(holder.values, vec![2.0, 7.0]);

        restore(&mut holder, history.undo());
        assert_eq!(holder.values, vec![1.0, 2.0, 7.0]);
        restore(&mut holder, history.undo());
        assert_eq!(holder.values, vec![1.0, 2.0, 0.0]);
        restore(&mut holder, history.undo());
        assert_eq!(holder.values, vec![1.0, 2.0]);
        assert!(!history.can_undo());

        restore(&mut holder, history.redo());
        restore(&mut holder, history.redo());
        restore(&mut holder, history.redo());
        assert_eq!(holder.values, vec![2.0, 7.0]);
    }

    #[test]
    fn list_edits_reject_out_of_range_removal() {
        let registry = TypeRegistry::new();
        let info = values_info(&registry);
        let mut values = vec![1.0f32];

        let list = values.as_reflect_mut().as_list_mut().unwrap();
        assert!(!apply_list_edit(
            &registry,
            Some(info),
            list,
            ListEdit::Remove(3)
        ));
        assert_eq!(values, vec![1.0]);
    }

    #[test]
    fn nested_lists_render_as_lists() {
        let registry = TypeRegistry::new();
        let inner = registry.type_info::<Vec<f32>>();
        assert!(matches!(inner, Some(TypeInfo::List(_))));

        assert_eq!(element_view(false, inner, true), ElementView::List);
        // An inner list type that no field registered still shows its elements.
        assert_eq!(element_view(false, None, true), ElementView::List);
    }

    #[test]
    fn list_elements_prefer_their_type_inspector() {
        let registry = TypeRegistry::new();
        let holder = registry.type_info::<ListHolder>();

        assert_eq!(element_view(true, None, false), ElementView::Inline);
        assert_eq!(element_view(false, holder, false), ElementView::Struct);
        assert_eq!(element_view(false, None, false), ElementView::Label);
    }

    #[test]
    fn lists_without_list_info_cannot_grow() {
        let registry = TypeRegistry::new();
        let mut values = vec![1.0f32];

        let list = values.as_reflect_mut().as_list_mut().unwrap();
        assert!(list_element_default(&registry, None).is_none());
        assert!(!apply_list_edit(&registry, None, list, ListEdit::Add));
        assert!(apply_list_edit(&registry, None, list, ListEdit::Remove(0)));
        assert!(values.is_empty());
    }

    #[test]
    fn list_resizes_are_told_apart_from_element_edits() {
        let edit = |before: Value, after: Value| InspectorValueEdit {
            label: String::new(),
            key: InspectorValueKey {
                game_object: Uuid::nil(),
                component: Uuid::nil(),
                value_path: vec!["values".into()],
            },
            before,
            after,
        };

        assert!(edit(json!([1.0]), json!([1.0, 0.0])).resizes_list());
        assert!(edit(json!([1.0, 2.0]), json!([2.0])).resizes_list());
        assert!(!edit(json!([1.0, 2.0]), json!([2.0, 1.0])).resizes_list());
        assert!(!edit(json!(1.0), json!(2.0)).resizes_list());
    }

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
