use std::any::{Any, TypeId};
use std::collections::HashSet;

use crate::inspector::assets::animation_graph_inspector::AnimationGraphInspector;
use crate::inspector::inspector_registry::InspectorRegistry;
use crate::inspector::type_inspector::InspectorContext;
use crate::inspector::widgets::Widgets;
use crate::panel::Panel;
use crate::selection::SelectionType;
use crate::EditorAppState;
use convert_case::{Case, Casing};
use engine::assets::animation_graph::AnimationGraph;
use engine::assets::AssetRegistry;
use engine::class_registry::ClassRegistry;
use engine::component::{ComponentID, ComponentTransform};
use engine::egui;
use engine::egui::scroll_area::ScrollBarVisibility;
use engine::egui::{Id, PopupCloseBehavior, Response, Ui};
use engine::reflect::type_registry::TypeRegistry;
use engine::reflect::{AttributeValue, NamedField, Reflect, TypeInfo};
use engine::scene::{GameObject, SceneManager};
use re_ui::list_item::{LabelContent, ListItem};
use re_ui::{DesignTokens, UiExt};

#[derive(Default)]
pub struct PanelInspector;

impl Panel for PanelInspector {
    fn name() -> &'static str {
        "Inspector"
    }

    fn ui(&mut self, ui: &mut Ui) {
        let app_state = EditorAppState::get();
        let registry = TypeRegistry::get();

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
                        if let Some(game_object) = app_state
                            .selection
                            .first(SelectionType::GameObject)
                            .and_then(|id| {
                                SceneManager::get()
                                    .simulation_scene()
                                    .get_game_object_by_uuid(id)
                            })
                        {
                            let mut entity_components = HashSet::new();
                            let mut components_to_remove = HashSet::new();

                            Self::add_component_button_ui(ui, &entity_components, game_object);

                            for (type_id, component) in ClassRegistry::get().components() {
                                let Some(instance) = SceneManager::get_mut()
                                    .simulation_scene_mut()
                                    .get_component_ptr(game_object, component)
                                    .map(|ptr| unsafe { &mut *ptr })
                                else {
                                    continue;
                                };

                                entity_components.insert(*type_id);
                                let Some(TypeInfo::Struct(type_info)) =
                                    registry.type_info_by_id(*type_id)
                                else {
                                    continue;
                                };
                                let scene_state = SceneManager::get();
                                let ctx = InspectorContext {
                                    registry: &registry,
                                    scene: scene_state.simulation_scene(),
                                    game_object,
                                    parent: SceneManager::get()
                                        .simulation_scene()
                                        .get_parent_game_object(game_object),
                                    type_info,
                                    field_name: None,
                                };
                                if self.show_inspector(ui, &ctx, instance.as_reflect_mut()) {
                                    components_to_remove.insert(*type_id);
                                }
                            }
                            for (type_id, component) in ClassRegistry::get().components() {
                                if !components_to_remove.contains(type_id) {
                                    continue;
                                }
                                if let Some(mut entry) = SceneManager::get_mut()
                                    .simulation_scene_mut()
                                    .entry_mut(game_object)
                                {
                                    component.remove_instance(&mut entry);
                                }
                            }
                        } else if let Some(asset_id) =
                            app_state.selection.first(SelectionType::Asset)
                        {
                            let Some(asset_meta) =
                                AssetRegistry::get().asset_meta_from_id(asset_id)
                            else {
                                return;
                            };
                            let Some((_, type_uuid, type_name)) = asset_meta.type_id else {
                                return;
                            };
                            let registry = InspectorRegistry::get();
                            let Some(inspector) = registry.asset_inspector_lookup(type_uuid) else {
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
                                    LabelContent::new(format!(
                                        "[{}] {}",
                                        type_name, asset_meta.name
                                    ))
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
                                                AssetRegistry::get().persist(asset_id);
                                            }
                                        });
                                        res
                                    }),
                                    |ui| {
                                        inspector.show_inspector(ui, asset_id);
                                    },
                                );
                        } else if let SelectionType::AnimationNode(asset_id) =
                            app_state.selection.ty()
                        {
                            if let Some(id) = app_state.selection.iter().next() {
                                if let Ok(graph_ref) =
                                    AssetRegistry::get().load_by_id::<AnimationGraph>(asset_id)
                                {
                                    let mut graph = graph_ref.write();
                                    AnimationGraphInspector::node(ui, &mut graph, id);
                                }
                            }
                        } else if let SelectionType::AnimationTransition(asset_id) =
                            app_state.selection.ty()
                        {
                            if let Some(id) = app_state.selection.iter().next() {
                                if let Ok(graph_ref) =
                                    AssetRegistry::get().load_by_id::<AnimationGraph>(asset_id)
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
    fn display_name(instance: &dyn Reflect) -> &'static str {
        let registry = TypeRegistry::get();
        registry
            .type_info_by_id(instance.type_id())
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
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
    ) -> bool {
        let name = Self::display_name(instance);
        let id = ui.make_persistent_id(name);
        let type_id = instance.as_any().type_id();
        let res = ListItem::new()
            .interactive(true)
            .force_background(re_ui::design_tokens().section_collapsing_header_color())
            .show_hierarchical_with_children_unindented(
                ui,
                id,
                true,
                LabelContent::new(name).truncate(true),
                |ui| {
                    if let Some(inspector) = InspectorRegistry::get().type_inspector_lookup(type_id)
                    {
                        inspector.show_inspector(ui, ctx, instance);
                    } else {
                        self.show_default_inspector(ui, ctx, instance);
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
        if type_id != TypeId::of::<ComponentID>() {
            res.context_menu(|ui| {
                if type_id != TypeId::of::<ComponentTransform>() && ui.button("Remove").clicked() {
                    remove = true;
                    ui.close_menu();
                }
                if let Some(inspector) = InspectorRegistry::get().type_inspector_lookup(type_id) {
                    inspector.show_inspector_context(ui, ctx, instance);
                }
            });
        }
        remove
    }

    fn show_default_inspector(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
    ) {
        if let Some(TypeInfo::Struct(info)) =
            ctx.registry.type_info_by_id(instance.as_any().type_id())
        {
            for (_, field) in info.fields.iter() {
                let mut ctx = *ctx;
                ctx.field_name = Some(field.name);
                if let Some(value) = field.get_reflect_mut(instance.as_reflect_mut()) {
                    self.show_default_inspector_field(ui, &ctx, field, value);
                }
            }
        }
    }

    fn show_default_inspector_field(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        field: &NamedField,
        instance: &mut dyn Reflect,
    ) {
        let mut name = Self::field_display_name(field);
        name.push(' ');
        if let Some(inspector) =
            InspectorRegistry::get().type_inspector_lookup(instance.as_any().type_id())
        {
            Widgets::inspector_prop_value(ui, name, |ui, _| {
                inspector.show_inspector(ui, ctx, instance);
            });
        }
    }

    fn add_component_button_ui(
        ui: &mut Ui,
        entity_components: &HashSet<TypeId>,
        game_object: GameObject,
    ) -> Response {
        let num_components = ClassRegistry::get().components().count();
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
            for (type_id, component) in ClassRegistry::get().components() {
                if entity_components.contains(type_id) {
                    continue;
                }
                let name = Self::display_name(component.as_reflect());
                if ui.selectable_label(false, name).clicked() {
                    SceneManager::get_mut()
                        .simulation_scene_mut()
                        .bind_component_dyn(game_object, *type_id);
                }
            }
        });
        if res.clicked() {
            ui.memory_mut(|mem| mem.open_popup(id));
        }
        res
    }
}
