use std::any::TypeId;
use std::collections::HashSet;

use convert_case::{Case, Casing};

use engine::assets::AssetRegistry;
use engine::class_registry::ClassRegistry;
use engine::component::{ComponentID, ComponentTransform};
use engine::egui::Ui;
use engine::egui_extras::{Column, TableBody};
use engine::reflect::type_registry::TypeRegistry;
use engine::reflect::{AttributeValue, Reflect, ReflectDefault, TypeInfo};
use engine::scene::SceneManager;

use engine::{egui, egui_extras};

use crate::inspector::inspector_registry::InspectorRegistry;
use crate::inspector::type_inspector::InspectorContext;
use crate::panel::Panel;
use crate::{EditorAppState, BASE_FONT_SIZE};

#[derive(Default)]
pub struct PanelInspector;

impl Panel for PanelInspector {
    fn name() -> &'static str {
        "Inspector"
    }

    fn ui(&mut self, ui: &mut Ui) {
        let app_state = EditorAppState::get();
        let registry = TypeRegistry::get();
        let selection = app_state.selection.clone();
        if let Some(game_object) =
            selection
                .as_ref()
                .and_then(|s| s.first_entity())
                .and_then(|id| {
                    SceneManager::get()
                        .simulation_scene()
                        .get_game_object_by_uuid(id)
                })
        {
            let mut entity_components = HashSet::new();
            let mut components_to_remove = HashSet::new();
            for (type_id, component) in ClassRegistry::get().components() {
                let ptr = SceneManager::get_mut()
                    .simulation_scene_mut()
                    .get_component_ptr(game_object, component);
                if let Some(instance) = ptr.map(|ptr| unsafe { &mut *ptr }) {
                    entity_components.insert(*type_id);
                    let info = registry.type_info_by_id(*type_id).unwrap();
                    if let TypeInfo::Struct(type_info) = info {
                        let scene_state = SceneManager::get();
                        let ctx = InspectorContext {
                            registry: &registry,
                            scene: scene_state.simulation_scene(),
                            game_object,
                            parent: SceneManager::get()
                                .simulation_scene()
                                .get_parent_game_object(game_object),
                            world: &scene_state.simulation_scene().world,
                            type_info,
                            field_name: None,
                        };
                        if self.show_inspector(ui, &ctx, instance.as_reflect_mut()) {
                            components_to_remove.insert(*type_id);
                        }
                    }
                }
            }

            let num_components = ClassRegistry::get().components().count();
            ui.add_enabled_ui(num_components > entity_components.len(), |ui| {
                let res = ui.add_sized(
                    [ui.available_width(), BASE_FONT_SIZE + 4.0],
                    egui::Button::new("Add Component"),
                );
                let id = ui.make_persistent_id("add_component_popup");
                egui::popup::popup_below_widget(ui, id, &res, |ui| {
                    for (type_id, component) in ClassRegistry::get().components() {
                        if entity_components.contains(type_id) {
                            continue;
                        }
                        let name = Self::display_name(component.as_reflect());
                        if ui.selectable_label(false, name).clicked() {
                            let meta = registry.trait_meta::<ReflectDefault>(*type_id).unwrap();
                            if let Some(mut entry) = SceneManager::get_mut()
                                .simulation_scene_mut()
                                .entry_mut(game_object)
                            {
                                component.bind_instance(&mut entry, meta.default());
                            }
                        }
                    }
                });
                if res.clicked() {
                    ui.memory_mut(|mem| mem.open_popup(id));
                }
            });

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
        } else if let Some(id) = selection.as_ref().and_then(|s| s.first_asset()) {
            let registry = AssetRegistry::get();
            if let Ok(asset) = registry.load_dyn_by_id(id) {
                if let Some(meta) = registry.asset_meta_from_id(id) {
                    if let Some(type_uuid) = meta.type_uuid {
                        if let Some(inspector) =
                            InspectorRegistry::get().asset_inspector_lookup(type_uuid)
                        {
                            ui.collapsing(registry.asset_name(id), |ui| {
                                inspector.show_inspector(ui, asset);
                                ui.separator();
                            });
                        }
                    }
                }
            }
        }
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

    fn show_inspector(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
    ) -> bool {
        let name = Self::display_name(instance);
        let mut remove = false;
        let type_id = instance.as_any().type_id();
        let res = egui::CollapsingHeader::new(name)
            .default_open(true)
            .show(ui, |ui| {
                if let Some(inspector) = InspectorRegistry::get().type_inspector_lookup(type_id) {
                    inspector.show_inspector(ui, ctx, instance);
                } else {
                    self.show_default_inspector(ui, ctx, instance);
                }
            })
            .header_response;
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
        ui.separator();
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
            egui_extras::TableBuilder::new(ui)
                .column(
                    Column::auto_with_initial_suggestion(200.0)
                        .clip(true)
                        .resizable(true),
                )
                .column(Column::remainder().clip(true))
                .body(|mut body| {
                    for (_, field) in info.fields.iter() {
                        let mut ctx = *ctx;
                        ctx.field_name = Some(field.name);
                        if let Some(value) = field.get_reflect_mut(instance.as_reflect_mut()) {
                            self.show_default_inspector_field(&mut body, &ctx, field.name, value);
                        }
                    }
                });
        }
    }

    fn show_default_inspector_field(
        &self,
        body: &mut TableBody,
        ctx: &InspectorContext,
        field_name: &str,
        instance: &mut dyn Reflect,
    ) {
        let mut name = field_name.from_case(Case::Snake).to_case(Case::Title);
        name.push(' ');
        if let Some(inspector) =
            InspectorRegistry::get().type_inspector_lookup(instance.as_any().type_id())
        {
            body.row(BASE_FONT_SIZE + 6.0, |mut row| {
                row.col(|ui| {
                    ui.add(egui::Label::new(name.as_str()).wrap(false));
                });
                row.col(|ui| {
                    inspector.show_inspector(ui, ctx, instance);
                });
            });
        }
    }
}
