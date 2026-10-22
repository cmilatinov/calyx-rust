use std::any::Any;
use std::collections::HashSet;

use crate::inspector::assets::animation_graph_inspector::AnimationGraphInspector;
use crate::inspector::inspector_registry::InspectorRegistry;
use crate::inspector::type_inspector::{InspectorContext, InspectorResponse};
use crate::inspector::widgets::{SearchSelectState, Widgets};
use crate::panel::Panel;
use crate::selection::SelectionType;
use crate::widgets::{List, ListContainer, ListItemContext};
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
use re_ui::list_item::{LabelContent, ListItem, PropertyContent, ShowCollapsingResponse};
use re_ui::{DesignTokens, UiExt};
use uuid::Uuid;

/// A structural change the inspector made to a reflected list.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ListEdit {
    /// Appended a default element.
    Add,
    /// Removed the element at this index.
    Remove(usize),
    /// Moved the element at `from` so it ended up at `to`.
    Move { from: usize, to: usize },
}

/// Adapts a reflected list to the [`List`] widget.
///
/// Adding appends the element type's registered default, and each
/// structural change is logged in `edits`, so the inspector reports it as a
/// discrete edit: one undo step per add, remove or move.
struct ReflectListContainer<'a> {
    list: &'a mut dyn ReflectList,
    default: Option<&'a ReflectDefault>,
    edits: Vec<ListEdit>,
}

impl<'a> ReflectListContainer<'a> {
    /// Wraps `list`. Without `info` the element type is unknown, so the list
    /// cannot grow.
    fn new(
        list: &'a mut dyn ReflectList,
        type_registry: &'a TypeRegistry,
        info: Option<&ListInfo>,
    ) -> Self {
        let default = info
            .and_then(|info| type_registry.trait_meta::<ReflectDefault>(info.element.type_uuid));
        Self {
            list,
            default,
            edits: Vec::new(),
        }
    }
}

impl ListContainer for ReflectListContainer<'_> {
    type Item = dyn Reflect;

    fn len(&self) -> usize {
        self.list.len()
    }

    fn nth(&self, index: usize) -> &Self::Item {
        self.list.element(index).expect("list index in range")
    }

    fn nth_mut(&mut self, index: usize) -> &mut Self::Item {
        self.list.element_mut(index).expect("list index in range")
    }

    fn can_add(&self) -> bool {
        self.default.is_some()
    }

    fn push_default(&mut self) {
        let Some(default) = self.default else {
            return;
        };
        if self.list.push(default.default()).is_ok() {
            self.edits.push(ListEdit::Add);
        }
    }

    fn remove(&mut self, index: usize) {
        if self.list.remove(index).is_some() {
            self.edits.push(ListEdit::Remove(index));
        }
    }

    fn move_item(&mut self, from: usize, to: usize) {
        let Some(item) = self.list.remove(from) else {
            return;
        };
        match self.list.insert(to, item) {
            Ok(()) => self.edits.push(ListEdit::Move { from, to }),
            Err(item) => {
                // Out of range: put the element back where it was.
                let _ = self.list.insert(from, item);
            }
        }
    }
}

/// Where a value sits in its component's serialized JSON.
struct FieldTrail {
    /// Identifies the component being inspected.
    base: Id,
    path: Vec<String>,
}

impl FieldTrail {
    /// The trail of a component's root value. `base` identifies the
    /// component, such as its game object and type.
    fn root(base: Id) -> Self {
        Self {
            base,
            path: Vec::new(),
        }
    }

    fn child(&self, segment: impl ToString) -> Self {
        let mut path = self.path.clone();
        path.push(segment.to_string());
        Self {
            base: self.base,
            path,
        }
    }

    /// A widget id unique to this value: its component and its full path,
    /// list indices included, so a field repeated in every element of a
    /// list gets its own collapse, selection and drag state per element.
    fn id(&self) -> Id {
        self.base.with(&self.path)
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
        state.inspector_rect = Some(ui.clip_rect());
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
                        let selected_game_object = state
                            .selection
                            .first(SelectionType::GameObject)
                            .and_then(|id| state.game.scenes.simulation_scene().find(id));
                        state.select_inspector_object(
                            selected_game_object
                                .map(|go| state.game.scenes.simulation_scene().uuid(go)),
                        );
                        if let Some(game_object) = selected_game_object {
                            // Play mode records no undo history, so the
                            // inspector keeps no baselines there.
                            let track_edits =
                                state.scene_document.track_inspector_edits(&state.game);
                            let pointer_down = ui.input(|input| input.pointer.any_down());
                            let mut structural_changed = false;
                            let mut structural_edit_before = None;
                            let mut entity_components = HashSet::new();
                            let mut components_to_remove = HashSet::new();
                            let component_registry_ref =
                                state.game.assets.registries.components.clone();
                            let component_registry = component_registry_ref.read();
                            let component_types =
                                state.game.scenes.simulation_scene().component_types(game_object);
                            for (type_id, component) in component_registry.components() {
                                if component_types.contains(&component.component_type_id()) {
                                    entity_components.insert(*type_id);
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
                                let assets = state.game.assets.lock_read();
                                let inspector_registry = &state.inspector_registry;
                                // SAFETY: inspectors read other objects and
                                // components through the scene, never the
                                // component they are editing.
                                let Some(shown) = (unsafe {
                                    state.scene_document.inspector_edits_mut().show_component(
                                        state.game.scenes.simulation_scene_mut(),
                                        game_object,
                                        &**component,
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
                                            self.show_inspector(
                                                ui,
                                                inspector_registry,
                                                &ctx,
                                                instance.as_reflect_mut(),
                                            )
                                        },
                                    )
                                }) else {
                                    continue;
                                };
                                drop(assets);
                                if shown.value {
                                    components_to_remove.insert(*type_id);
                                }
                                state.scene_document.inspector_shown(
                                    &state.game,
                                    shown.changed,
                                    shown.completed,
                                    shown.committed,
                                );
                            }
                            for (type_id, _) in component_registry.components() {
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
                                if state
                                    .game
                                    .scenes
                                    .simulation_scene_mut()
                                    .remove_component_dyn(game_object, *type_id)
                                {
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
    ) -> (InspectorResponse, bool) {
        let name = Self::display_name(&ctx.assets.types.read(), instance);
        let id = ui.make_persistent_id(name);
        let type_uuid = instance.uuid();
        let mut response = InspectorResponse::NONE;
        let res = ListItem::new()
            .interactive(true)
            .force_background(re_ui::design_tokens().section_collapsing_header_color())
            .show_hierarchical_with_children_unindented(
                ui,
                id,
                true,
                LabelContent::new(name).truncate(true),
                |ui| {
                    response |= match registry.type_inspector_lookup(type_uuid) {
                        Some(inspector) => inspector.show_inspector(ui, ctx, instance),
                        None => self.show_default_inspector(
                            ui,
                            registry,
                            ctx,
                            instance,
                            &FieldTrail::root(Id::new((ctx.game_object.node, type_uuid))),
                        ),
                    };
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
                    response |= inspector.show_inspector_context(ui, ctx, instance);
                }
            });
        }
        (response, remove)
    }

    fn show_default_inspector(
        &self,
        ui: &mut Ui,
        registry: &InspectorRegistry,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
        trail: &FieldTrail,
    ) -> InspectorResponse {
        let type_registry = ctx.assets.types.read();
        match type_registry.type_info_by_id(instance.uuid()) {
            Some(TypeInfo::Struct(info)) => {
                self.show_default_struct_inspector(ui, registry, ctx, instance, info, trail)
            }
            _ => InspectorResponse::NONE,
        }
    }

    fn show_default_struct_inspector(
        &self,
        ui: &mut Ui,
        registry: &InspectorRegistry,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
        info: &StructInfo,
        trail: &FieldTrail,
    ) -> InspectorResponse {
        let mut response = InspectorResponse::NONE;
        for (_, field) in info.fields.iter() {
            let mut field_ctx = *ctx;
            field_ctx.field_name = Some(field.name);
            if let Some(value) = field.get_reflect_mut(instance.as_reflect_mut()) {
                response |= self.show_default_inspector_field(
                    ui,
                    registry,
                    &field_ctx,
                    field,
                    value,
                    &trail.child(field.name),
                );
            }
        }
        response
    }

    fn show_default_inspector_field(
        &self,
        ui: &mut Ui,
        registry: &InspectorRegistry,
        ctx: &InspectorContext,
        field: &NamedField,
        instance: &mut dyn Reflect,
        trail: &FieldTrail,
    ) -> InspectorResponse {
        let mut name = Self::field_display_name(field);
        name.push(' ');
        let type_registry = ctx.assets.types.read();
        if let Some(inspector) = registry.type_inspector_lookup(instance.uuid()) {
            if matches!(
                type_registry.type_info_by_id(instance.uuid()),
                Some(TypeInfo::Enum(_))
            ) {
                return inspector.show_inspector(ui, ctx, instance);
            }
            let mut response = InspectorResponse::NONE;
            Widgets::inspector_prop_value(ui, name, |ui, _| {
                response = inspector.show_inspector(ui, ctx, instance);
            });
            return response;
        }
        let id = trail.id();
        match type_registry.type_info_by_id(instance.uuid()) {
            Some(TypeInfo::Struct(info)) => {
                let mut nested_ctx = *ctx;
                nested_ctx.field_name = None;
                nested_ctx.type_info = info;
                let mut response = InspectorResponse::NONE;
                ListItem::new()
                    .interactive(false)
                    .show_hierarchical_with_children(
                        ui,
                        id,
                        true,
                        PropertyContent::new(name).show_only_when_collapsed(false),
                        |ui| {
                            response = self.show_default_struct_inspector(
                                ui,
                                registry,
                                &nested_ctx,
                                instance,
                                info,
                                trail,
                            );
                        },
                    );
                response
            }
            Some(TypeInfo::List(info)) => self.show_list_inspector(
                ui,
                registry,
                ctx,
                &type_registry,
                name.trim_end(),
                id,
                instance,
                Some(info),
                trail,
            ),
            Some(_) => {
                Widgets::inspector_prop_value(ui, name, |ui, _| {
                    ui.label(instance.type_name_short());
                });
                InspectorResponse::NONE
            }
            None => InspectorResponse::NONE,
        }
    }

    /// Shows a list with the editor's [`List`] widget: a collapsing header
    /// with the element count and add/remove buttons, selectable elements and
    /// drag-and-drop reordering. An add, remove or move is reported as a
    /// discrete change, so it is one undo step; element edits report as their
    /// inspectors do.
    #[allow(clippy::too_many_arguments)]
    fn show_list_inspector(
        &self,
        ui: &mut Ui,
        registry: &InspectorRegistry,
        ctx: &InspectorContext,
        type_registry: &TypeRegistry,
        name: &str,
        id: Id,
        instance: &mut dyn Reflect,
        info: Option<&ListInfo>,
        trail: &FieldTrail,
    ) -> InspectorResponse {
        let Some(list) = instance.as_list_mut() else {
            return InspectorResponse::NONE;
        };
        let mut container = ReflectListContainer::new(list, type_registry, info);
        let mut response = InspectorResponse::NONE;
        List::new(name, &mut container, |_| ()).id(id).show(
            ui,
            |ui,
             ListItemContext {
                 value,
                 index,
                 selected,
             }| {
                let (element, shown) = ui
                    .push_id(index, |ui| {
                        self.show_list_element(
                            ui,
                            registry,
                            ctx,
                            type_registry,
                            id.with(index),
                            index,
                            selected,
                            value,
                            &trail.child(index),
                        )
                    })
                    .inner;
                response |= element;
                shown
            },
        );
        if !container.edits.is_empty() {
            response |= InspectorResponse::CHANGED;
        }
        response
    }

    /// Shows one list element as a selectable, draggable row.
    #[allow(clippy::too_many_arguments)]
    fn show_list_element(
        &self,
        ui: &mut Ui,
        registry: &InspectorRegistry,
        ctx: &InspectorContext,
        type_registry: &TypeRegistry,
        id: Id,
        index: usize,
        selected: bool,
        element: &mut dyn Reflect,
        trail: &FieldTrail,
    ) -> (InspectorResponse, ShowCollapsingResponse<()>) {
        let label = format!("Element {index}");
        let element_info = type_registry.type_info_by_id(element.uuid());
        let inspector = registry.type_inspector_lookup(element.uuid());
        let view = element_view(
            inspector.is_some(),
            element_info,
            element.as_list().is_some(),
        );
        let item = ListItem::new()
            .selected(selected)
            .interactive(true)
            .draggable(true);
        let mut response = InspectorResponse::NONE;
        if let (ElementView::Inline, Some(inspector)) = (view, inspector) {
            let item_response = item.show_flat(
                ui,
                PropertyContent::new(label)
                    .value_fn(|ui, _| response = inspector.show_inspector(ui, ctx, element)),
            );
            let shown = ShowCollapsingResponse {
                item_response,
                body_response: None,
                openness: 0.0,
            };
            return (response, shown);
        }
        let shown = item.show_hierarchical_with_children(
            ui,
            id,
            true,
            PropertyContent::new(label).show_only_when_collapsed(false),
            |ui| match (view, inspector, element_info) {
                (ElementView::Enum, Some(inspector), _) => {
                    response = inspector.show_inspector(ui, ctx, element);
                }
                (ElementView::Struct, _, Some(TypeInfo::Struct(info))) => {
                    let mut nested_ctx = *ctx;
                    nested_ctx.field_name = None;
                    nested_ctx.type_info = info;
                    response = self.show_default_struct_inspector(
                        ui,
                        registry,
                        &nested_ctx,
                        element,
                        info,
                        trail,
                    );
                }
                (ElementView::List, _, info) => {
                    let info = match info {
                        Some(TypeInfo::List(info)) => Some(info),
                        _ => None,
                    };
                    response = self.show_list_inspector(
                        ui,
                        registry,
                        ctx,
                        type_registry,
                        "Elements",
                        id.with("elements"),
                        element,
                        info,
                        trail,
                    );
                }
                _ => {
                    Widgets::inspector_prop_value(ui, "Value ", |ui, _| {
                        ui.label(element.type_name_short());
                    });
                }
            },
        );
        (response, shown)
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

    pub(crate) fn type_display_name(
        type_registry: &TypeRegistry,
        type_uuid: Uuid,
    ) -> Option<String> {
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
        element_view, ElementView, FieldTrail, ListEdit, PanelInspector, ReflectListContainer,
    };
    use crate::inspector::edit_tracker::{ComponentKey, InspectorEditTracker};
    use crate::inspector::test_harness::{any_changed, combined, InspectorHarness};
    use crate::inspector::type_inspector::{InspectorContext, InspectorResponse};
    use crate::scene_history::{InspectorValueEdit, SceneHistory, SceneHistoryRestore};
    use crate::widgets::ListContainer;
    use egui::accesskit::Role;
    use egui::{Id, Ui, Vec2};
    use engine::component::ComponentCamera;
    use engine::reflect::type_registry::TypeRegistry;
    use engine::reflect::{ListInfo, Reflect, TypeInfo};
    use engine::utils::TypeUuid;
    use serde::{Deserialize, Serialize};
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

    /// Runs `edit` on `holder`'s `values` list the way the inspector does:
    /// through the list widget's container, reported as one discrete change
    /// to the edit tracker, whose commit becomes the undo step. Returns how
    /// many history entries the edit added.
    fn inspector_edit(
        registry: &TypeRegistry,
        history: &mut SceneHistory,
        holder: &mut ListHolder,
        edit: impl FnOnce(&mut ReflectListContainer),
    ) -> usize {
        let key = ComponentKey {
            game_object: Uuid::nil(),
            component: ListHolder::type_uuid(),
        };
        let mut tracker = InspectorEditTracker::default();
        tracker.set_baseline(key, serde_json::to_value(&*holder).ok());
        let TypeInfo::Struct(info) = registry.type_info::<ListHolder>().unwrap() else {
            panic!("expected struct info");
        };
        let field = info.field("values").unwrap();
        let values = field.get_reflect_mut(holder.as_reflect_mut()).unwrap();
        let mut container = ReflectListContainer::new(
            values.as_list_mut().unwrap(),
            registry,
            Some(values_info(registry)),
        );
        edit(&mut container);
        // Element edits report as changes too; structural ones always do.
        assert!(tracker.report(key, InspectorResponse::CHANGED, false));
        let Some(commit) = tracker.commit(key, serde_json::to_value(&*holder).ok()) else {
            return 0;
        };
        assert!(history.push_inspector_value(InspectorValueEdit {
            label: "Edit List Holder".into(),
            game_object: commit.key.game_object,
            component: commit.key.component,
            changes: commit.changes,
        }));
        1
    }

    fn restore(holder: &mut ListHolder, restore: Option<SceneHistoryRestore>) {
        let Some(SceneHistoryRestore::InspectorValue { values, .. }) = restore else {
            panic!("expected an inspector value restore");
        };
        let mut serialized = serde_json::to_value(&*holder).unwrap();
        for (path, value) in values {
            assert!(crate::set_json_value(&mut serialized, &path, value));
        }
        *holder = serde_json::from_value(serialized).unwrap();
    }

    #[test]
    fn list_edits_round_trip_through_inspector_history() {
        let registry = TypeRegistry::new();
        let mut history = SceneHistory::default();
        let mut holder = ListHolder {
            values: vec![1.0, 2.0],
        };

        let added = inspector_edit(&registry, &mut history, &mut holder, |list| {
            assert!(list.can_add());
            list.push_default();
        });
        assert_eq!(added, 1);
        assert_eq!(holder.values, vec![1.0, 2.0, 0.0]);

        inspector_edit(&registry, &mut history, &mut holder, |list| {
            *list.nth_mut(2).downcast_mut::<f32>().unwrap() = 7.0;
        });
        assert_eq!(holder.values, vec![1.0, 2.0, 7.0]);

        let removed = inspector_edit(&registry, &mut history, &mut holder, |list| {
            list.remove(0);
        });
        assert_eq!(removed, 1);
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
    fn list_moves_are_one_undo_step() {
        let registry = TypeRegistry::new();
        let mut history = SceneHistory::default();
        let mut holder = ListHolder {
            values: vec![1.0, 2.0, 3.0],
        };

        // A move shifts every element between its ends, all in one step.
        let moved = inspector_edit(&registry, &mut history, &mut holder, |list| {
            list.move_item(0, 2);
        });
        assert_eq!(moved, 1);
        assert_eq!(holder.values, vec![2.0, 3.0, 1.0]);

        restore(&mut holder, history.undo());
        assert_eq!(holder.values, vec![1.0, 2.0, 3.0]);
        assert!(!history.can_undo());
        restore(&mut holder, history.redo());
        assert_eq!(holder.values, vec![2.0, 3.0, 1.0]);
    }

    #[test]
    fn list_inspector_reports_element_edits_and_structural_changes() {
        let mut h = InspectorHarness::new();
        h.type_info = ListHolder::type_uuid();
        let registry = h.inspector_registry();
        let panel = PanelInspector::default();
        let mut holder = ListHolder {
            values: vec![1.0, 2.0],
        };
        let mut draw = |ui: &mut Ui, ctx: &InspectorContext| {
            panel.show_default_inspector(
                ui,
                &registry,
                ctx,
                holder.as_reflect_mut(),
                &FieldTrail::root(Id::new("list_holder")),
            )
        };

        let idle = h.idle(3, &mut draw);
        assert!(idle.iter().all(InspectorResponse::is_none), "{idle:?}");
        let element = h.widget(Role::SpinButton, 0);
        let drag = combined(&h.drag(element, Vec2::new(40.0, 0.0), &mut draw));
        assert!(
            drag.changed && drag.drag_started && drag.drag_stopped,
            "{drag:?}"
        );
        assert!(!any_changed(&h.idle(2, &mut draw)));
        assert_ne!(holder.values[0], 1.0);
        assert_eq!(holder.values.len(), 2);
    }

    #[test]
    fn reflect_list_container_adds_removes_and_moves() {
        let registry = TypeRegistry::new();
        let info = values_info(&registry);
        let mut values = vec![1.0f32, 2.0];
        let list = values.as_reflect_mut().as_list_mut().unwrap();
        let mut container = ReflectListContainer::new(list, &registry, Some(info));

        container.push_default();
        container.move_item(2, 0);
        container.remove(1);
        // Out of range: nothing changes and nothing is logged.
        container.remove(9);
        container.move_item(0, 9);

        assert_eq!(container.len(), 2);
        assert_eq!(
            container.edits,
            vec![
                ListEdit::Add,
                ListEdit::Move { from: 2, to: 0 },
                ListEdit::Remove(1)
            ]
        );
        assert_eq!(values, vec![0.0, 2.0]);
    }

    #[test]
    fn lists_without_list_info_cannot_grow() {
        let registry = TypeRegistry::new();
        let mut values = vec![1.0f32];
        let list = values.as_reflect_mut().as_list_mut().unwrap();
        let mut container = ReflectListContainer::new(list, &registry, None);

        assert!(!container.can_add());
        container.push_default();
        container.remove(0);

        assert_eq!(container.edits, vec![ListEdit::Remove(0)]);
        assert!(values.is_empty());
    }

    #[test]
    fn field_ids_differ_per_list_element() {
        let component = FieldTrail::root(Id::new("component"));
        let rows = component.child("rows");
        let first = rows.child(0).child("slots");
        let second = rows.child(1).child("slots");

        assert_ne!(first.id(), second.id());
        assert_eq!(first.id(), rows.child(0).child("slots").id());
        let other = FieldTrail::root(Id::new("other"));
        assert_ne!(first.id(), other.child("rows").child(0).child("slots").id());
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
    fn default_inspector_reports_its_fields_edits() {
        let mut h = InspectorHarness::new();
        h.type_info = ComponentCamera::type_uuid();
        let registry = h.inspector_registry();
        let panel = PanelInspector::default();
        let mut camera = ComponentCamera::default();
        let before = serde_json::to_value(&camera).unwrap();
        let mut draw = |ui: &mut Ui, ctx: &InspectorContext| {
            let (response, remove) =
                panel.show_inspector(ui, &registry, ctx, camera.as_reflect_mut());
            assert!(!remove);
            response
        };

        let idle = h.idle(3, &mut draw);
        assert!(idle.iter().all(|r| r.is_none()), "{idle:?}");
        let field = h.widget(Role::SpinButton, 0);
        let drag = combined(&h.drag(field, Vec2::new(40.0, 0.0), &mut draw));
        assert!(
            drag.changed && drag.drag_started && drag.drag_stopped,
            "{drag:?}"
        );
        let checkbox = h.widget(Role::CheckBox, 0);
        let click = h.click(checkbox, &mut draw);
        assert_eq!(click.iter().filter(|r| r.changed).count(), 1, "{click:?}");
        assert!(!any_changed(&h.idle(2, &mut draw)));

        let after = serde_json::to_value(&camera).unwrap();
        assert_ne!(before["enabled"], after["enabled"]);
        let numbers_changed = ["fov", "near_plane", "far_plane"]
            .iter()
            .filter(|field| before[**field] != after[**field])
            .count();
        assert_eq!(numbers_changed, 1, "{before} -> {after}");
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
}
