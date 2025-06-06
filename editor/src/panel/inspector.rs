use std::any::Any;
use std::collections::HashSet;

use crate::inspector::assets::animation_graph_inspector::AnimationGraphInspector;
use crate::inspector::inspector_registry::InspectorRegistry;
use crate::inspector::type_inspector::InspectorContext;
use crate::inspector::widgets::Widgets;
use crate::panel::Panel;
use crate::selection::SelectionType;
use crate::EditorAppState;
use convert_case::{Case, Casing};
use egui::scroll_area::ScrollBarVisibility;
use egui::{Id, PopupCloseBehavior, Response, Ui};
use engine::assets::animation_graph::AnimationGraph;
use engine::component::{ComponentID, ComponentTransform};
use engine::context::ReadOnlyAssetContext;
use engine::reflect::type_registry::TypeRegistry;
use engine::reflect::{AttributeValue, NamedField, Reflect, TypeInfo};
use engine::scene::{GameObject, SceneManager};
use engine::utils::TypeUuid;
use re_ui::list_item::{LabelContent, ListItem};
use re_ui::{DesignTokens, UiExt};
use uuid::Uuid;

#[derive(Default)]
pub struct PanelInspector;

impl Panel for PanelInspector {
    fn name() -> &'static str {
        "Inspector"
    }

    fn ui(&mut self, ui: &mut Ui, state: &mut EditorAppState) {
        let type_registry_ref = state.game.assets.type_registry.clone();
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
                            .and_then(|id| {
                                state
                                    .game
                                    .scenes
                                    .simulation_scene()
                                    .get_game_object_by_uuid(id)
                            })
                        {
                            let mut entity_components = HashSet::new();
                            let mut components_to_remove = HashSet::new();

                            Self::add_component_button_ui(
                                ui,
                                &state.game.assets.lock_read(),
                                &mut state.game.scenes,
                                &entity_components,
                                game_object,
                            );

                            let component_registry_ref =
                                state.game.assets.component_registry.clone();
                            let component_registry = component_registry_ref.read();
                            for (type_id, component) in component_registry.components() {
                                let Some(instance) = (unsafe {
                                    state
                                        .game
                                        .scenes
                                        .simulation_scene_mut()
                                        .get_component_ptr(game_object, &**component)
                                        .map(|ptr| &mut *ptr)
                                }) else {
                                    continue;
                                };

                                entity_components.insert(*type_id);
                                let Some(TypeInfo::Struct(type_info)) =
                                    type_registry.type_info_by_id(*type_id)
                                else {
                                    continue;
                                };
                                let simulation_scene = state.game.scenes.simulation_scene();
                                let ctx = InspectorContext {
                                    assets: &state.game.assets.lock_read(),
                                    scene: simulation_scene,
                                    game_object,
                                    parent: simulation_scene.get_parent_game_object(game_object),
                                    type_info,
                                    field_name: None,
                                };
                                if self.show_inspector(
                                    ui,
                                    &state.inspector_registry,
                                    &ctx,
                                    instance.as_reflect_mut(),
                                ) {
                                    components_to_remove.insert(*type_id);
                                }
                            }
                            for (type_id, component) in component_registry.components() {
                                if !components_to_remove.contains(type_id) {
                                    continue;
                                }
                                if let Some(mut entry) = state
                                    .game
                                    .scenes
                                    .simulation_scene_mut()
                                    .entry_mut(game_object)
                                {
                                    component.remove_instance(&mut entry);
                                }
                            }
                        } else if let Some(asset_id) = state.selection.first(SelectionType::Asset) {
                            let asset_registry_ref = state.game.assets.asset_registry.clone();
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
                                    .asset_registry
                                    .read()
                                    .load_by_id::<AnimationGraph>(asset_id)
                                {
                                    let mut graph = graph_ref.write();
                                    let asset_registry = state.game.assets.asset_registry.read();
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
                                    .asset_registry
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
    fn display_name(type_registry: &TypeRegistry, instance: &dyn Reflect) -> &'static str {
        type_registry
            .type_info_by_id(instance.uuid())
            .and_then(|info| {
                if let TypeInfo::Struct(info) = info {
                    if let Some(AttributeValue::String(str)) = info.attr("name") {
                        return Some(str);
                    }
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
        let name = Self::display_name(&ctx.assets.type_registry.read(), instance);
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
        if let Some(TypeInfo::Struct(info)) = ctx
            .assets
            .type_registry
            .read()
            .type_info_by_id(instance.uuid())
        {
            for (_, field) in info.fields.iter() {
                let mut ctx = *ctx;
                ctx.field_name = Some(field.name);
                if let Some(value) = field.get_reflect_mut(instance.as_reflect_mut()) {
                    self.show_default_inspector_field(ui, registry, &ctx, field, value);
                }
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
        if let Some(inspector) = registry.type_inspector_lookup(instance.uuid()) {
            Widgets::inspector_prop_value(ui, name, |ui, _| {
                inspector.show_inspector(ui, ctx, instance);
            });
        }
    }

    fn add_component_button_ui(
        ui: &mut Ui,
        assets: &ReadOnlyAssetContext,
        scenes: &mut SceneManager,
        entity_components: &HashSet<Uuid>,
        game_object: GameObject,
    ) -> Response {
        let num_components = assets.component_registry.read().components().count();
        let res = ui
            .list_item()
            .draggable(false)
            .interactive(num_components > entity_components.len())
            .show_flat(
                ui,
                LabelContent::new(" Add Component")
                    .always_show_buttons(true)
                    .truncate(true)
                    .with_icon(&re_ui::icons::ADD),
            )
            .on_hover_text("Add a new component to this game object");
        let id = ui.make_persistent_id("add_component_popup");
        egui::popup::popup_below_widget(ui, id, &res, PopupCloseBehavior::CloseOnClick, |ui| {
            for (type_uuid, component) in assets.component_registry.read().components() {
                if entity_components.contains(type_uuid) {
                    continue;
                }
                let name = Self::display_name(&assets.type_registry.read(), component.as_reflect());
                if ui.selectable_label(false, name).clicked() {
                    scenes
                        .simulation_scene_mut()
                        .bind_component_dyn(game_object, *type_uuid);
                }
            }
        });
        if res.clicked() {
            ui.memory_mut(|mem| mem.open_popup(id));
        }
        res
    }
}
