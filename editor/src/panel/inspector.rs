use std::collections::{HashMap, HashSet};

use convert_case::{Case, Casing};

use engine::class_registry::ClassRegistry;
use engine::component::Component;
use engine::egui::Ui;
use engine::egui_extras::{Column, TableBody};
use engine::type_registry::TypeRegistry;
use engine::utils::type_uuids;
use engine::uuid::Uuid;
use engine::{egui, egui_extras};
use reflect::{AttributeValue, Reflect, ReflectDefault, TypeInfo, TypeUuid};

use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use crate::panel::Panel;
use crate::{EditorAppState, BASE_FONT_SIZE};

pub struct PanelInspector {
    inspectors: HashMap<Uuid, Box<dyn TypeInspector>>,
    type_association: HashMap<Uuid, Uuid>,
}

impl Default for PanelInspector {
    fn default() -> Self {
        let registry = TypeRegistry::get();
        let mut inspectors = HashMap::new();
        let mut type_association = HashMap::new();
        for type_id in registry.all_of(type_uuids!(ReflectDefault, ReflectTypeInspector)) {
            let meta_default = registry.trait_meta::<ReflectDefault>(type_id).unwrap();
            let meta_inspector = registry
                .trait_meta::<ReflectTypeInspector>(type_id)
                .unwrap();
            let instance = meta_default.default();
            let inspector = meta_inspector.get_boxed(instance).unwrap();
            for target_type_id in inspector.target_type_uuids() {
                type_association.insert(target_type_id, type_id);
            }
            inspectors.insert(type_id, inspector);
        }
        Self {
            inspectors,
            type_association,
        }
    }
}

impl Panel for PanelInspector {
    fn name() -> &'static str {
        "Inspector"
    }

    fn ui(&mut self, ui: &mut Ui) {
        let app_state = EditorAppState::get();
        let registry = TypeRegistry::get();
        if let Some(node) = app_state.selection.clone().and_then(|s| s.first_entity()) {
            let entity = app_state.scene.get_entity(node);
            let mut entity_components = HashSet::new();
            let mut components_to_remove = HashSet::new();
            for (type_id, component) in ClassRegistry::get().components() {
                let mut ptr = None;
                if let Some(mut entry) = app_state.scene.world_mut().entry(entity) {
                    if let Some(instance) = component.get_instance_mut(&mut entry) {
                        ptr = Some(instance as *mut _);
                        entity_components.insert(*type_id);
                    }
                }
                if let Some(ptr) = ptr {
                    let world = app_state.scene.world();
                    let instance: &mut dyn Component = unsafe { &mut *ptr };
                    let info = registry.type_info_by_uuid(*type_id).unwrap();
                    if let TypeInfo::Struct(type_info) = info {
                        let ctx = InspectorContext {
                            registry: &registry,
                            scene: &app_state.scene,
                            node,
                            parent_node: app_state.scene.get_parent_node(node),
                            world: &world,
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
                            let mut world = app_state.scene.world_mut();
                            if let Some(mut entry) = world.entry(entity) {
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
                let mut world = app_state.scene.world_mut();
                if let Some(mut entry) = world.entry(entity) {
                    component.remove_instance(&mut entry);
                }
            }
        }
    }
}

impl PanelInspector {
    fn display_name(instance: &dyn Reflect) -> &'static str {
        let registry = TypeRegistry::get();
        registry
            .type_info_by_uuid(instance.uuid())
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

    fn inspector_lookup(&self, type_id: Uuid) -> Option<&dyn TypeInspector> {
        match self.type_association.get(&type_id) {
            Some(inspector_id) => match self.inspectors.get(inspector_id) {
                Some(inspector) => Some(inspector.as_ref()),
                None => None,
            },
            None => match self.inspectors.get(&type_id) {
                Some(inspector) => Some(inspector.as_ref()),
                None => None,
            },
        }
    }

    fn show_inspector(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
    ) -> bool {
        let name = Self::display_name(instance);
        let mut remove = false;
        match self.inspector_lookup(instance.uuid()) {
            Some(inspector) => {
                ui.collapsing(name, |ui| {
                    inspector.show_inspector(ui, ctx, instance);
                })
                .header_response
                .context_menu(|ui| {
                    if ui.button("Remove").clicked() {
                        remove = true;
                        ui.close_menu();
                    }
                    inspector.show_inspector_context(ui, ctx, instance);
                });
                ui.separator();
            }
            None => {
                ui.collapsing(name, |ui| {
                    self.show_default_inspector(ui, ctx, instance);
                })
                .header_response
                .context_menu(|ui| {
                    if ui.button("Remove").clicked() {
                        remove = true;
                        ui.close_menu();
                    }
                });
                ui.separator();
            }
        };
        remove
    }

    fn show_default_inspector(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
    ) {
        if let Some(TypeInfo::Struct(info)) = ctx.registry.type_info_by_uuid(instance.uuid()) {
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
        if let Some(inspector) = self.inspector_lookup(instance.uuid()) {
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
