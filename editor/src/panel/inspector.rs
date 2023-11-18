use std::any::TypeId;
use std::collections::HashMap;

use engine::class_registry::ClassRegistry;
use engine::egui::Ui;
use engine::egui_extras;
use engine::egui_extras::{Column, TableBody};
use engine::legion::EntityStore;
use reflect::type_registry::TypeRegistry;
use reflect::{AttributeValue, Reflect, ReflectDefault, TypeInfo};
use utils::type_ids;

use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use crate::panel::Panel;
use crate::{EditorAppState, BASE_FONT_SIZE};

pub struct PanelInspector {
    inspectors: HashMap<TypeId, Box<dyn TypeInspector>>,
    type_association: HashMap<TypeId, TypeId>,
}

impl Default for PanelInspector {
    fn default() -> Self {
        let registry = TypeRegistry::get();
        let mut inspectors = HashMap::new();
        let mut type_association = HashMap::new();
        for type_id in registry.all_of(type_ids!(ReflectDefault, ReflectTypeInspector)) {
            let meta_default = registry.trait_meta::<ReflectDefault>(type_id).unwrap();
            let meta_inspector = registry
                .trait_meta::<ReflectTypeInspector>(type_id)
                .unwrap();
            let instance = meta_default.default();
            let inspector = meta_inspector.get_boxed(instance).unwrap();
            for target_type_id in inspector.target_type_ids() {
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
            let mut changes = HashMap::new();
            {
                let world = app_state.scene.world();
                if let Ok(entry) = world.entry_ref(entity) {
                    for component in ClassRegistry::get().components().iter() {
                        let ctx = InspectorContext {
                            registry: &registry,
                            scene: &app_state.scene,
                            node,
                            parent_node: app_state.scene.get_parent_node(node),
                            world: &world,
                        };
                        if let Some(instance) = component.get_instance(&entry) {
                            let mut copy = instance.cloned();
                            self.show_inspector(ui, &ctx, &mut *copy);
                            changes.insert(component.as_any().type_id(), copy);
                        }
                    }
                }
            }

            let mut world = app_state.scene.world_mut();
            for component in ClassRegistry::get().components().iter() {
                if let Some(value) = changes.remove(&component.as_any().type_id()) {
                    if let Some(mut entry) = world.entry(entity) {
                        if let Some(instance) = component.get_instance_mut(&mut entry) {
                            instance.assign(value);
                        }
                    }
                }
            }

            if ui.button("Add Component").clicked() {}
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

    fn inspector_lookup(&self, type_id: TypeId) -> Option<&dyn TypeInspector> {
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

    fn show_inspector(&self, ui: &mut Ui, ctx: &InspectorContext, instance: &mut dyn Reflect) {
        let name = Self::display_name(instance);
        match self.inspector_lookup(instance.as_any().type_id()) {
            Some(inspector) => {
                ui.collapsing(name, |ui| {
                    inspector.show_inspector(ui, ctx, instance);
                })
                .header_response
                .context_menu(|ui| {
                    inspector.show_inspector_context(ui, ctx, instance);
                });
                ui.separator();
            }
            None => {
                ui.collapsing(name, |ui| {
                    self.show_default_inspector(ui, ctx, instance);
                });
                ui.separator();
            }
        };
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
                .column(Column::auto().clip(true).resizable(true))
                .column(Column::remainder().clip(true))
                .body(|mut body| {
                    for (_, field) in info.fields.iter() {
                        if let Some(value) = field.get_reflect_mut(instance.as_reflect_mut()) {
                            self.show_default_inspector_field(&mut body, ctx, field.name, value);
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
        if let Some(inspector) = self.inspector_lookup(instance.as_any().type_id()) {
            body.row(BASE_FONT_SIZE + 6.0, |mut row| {
                row.col(|ui| {
                    ui.label(format!("{} ", field_name));
                });
                row.col(|ui| {
                    inspector.show_inspector(ui, ctx, instance);
                });
            });
        }
    }
}
