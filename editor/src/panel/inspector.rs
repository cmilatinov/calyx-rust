use std::any::TypeId;
use std::collections::{HashMap, HashSet};

use convert_case::{Case, Casing};

use engine::assets::AssetRegistry;
use engine::class_registry::ClassRegistry;
use engine::component::Component;
use engine::egui::Ui;
use engine::egui_extras::{Column, TableBody};
use engine::reflect::type_registry::TypeRegistry;
use engine::reflect::{AttributeValue, Reflect, ReflectDefault, TypeInfo};
use engine::uuid::Uuid;
use engine::{egui, egui_extras, type_ids};

use crate::inspector::asset_inspector::{AssetInspector, ReflectAssetInspector};
use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use crate::panel::Panel;
use crate::{EditorAppState, BASE_FONT_SIZE};

// TODO: Move this state into its own `InspectorRegistry`
// singleton with a dynamically registered listener to
// grab its data from the TypeRegistry when ClassRegistry's
// refresh_class_lists is called
pub struct PanelInspector {
    type_inspectors: HashMap<TypeId, Box<dyn TypeInspector>>,
    type_association: HashMap<TypeId, TypeId>,
    asset_inspectors: HashMap<Uuid, Box<dyn AssetInspector>>,
}

impl Default for PanelInspector {
    fn default() -> Self {
        let registry = TypeRegistry::get();
        let mut type_inspectors = HashMap::new();
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
            type_inspectors.insert(type_id, inspector);
        }

        let mut asset_inspectors = HashMap::new();
        for type_id in registry.all_of(type_ids!(ReflectDefault, ReflectAssetInspector)) {
            let meta_default = registry.trait_meta::<ReflectDefault>(type_id).unwrap();
            let meta_inspector = registry
                .trait_meta::<ReflectAssetInspector>(type_id)
                .unwrap();
            let instance = meta_default.default();
            let inspector = meta_inspector.get_boxed(instance).unwrap();
            asset_inspectors.insert(inspector.target_type_uuid(), inspector);
        }

        Self {
            type_inspectors,
            type_association,
            asset_inspectors,
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
        let selection = app_state.selection.clone();
        if let Some(node) = selection.as_ref().and_then(|s| s.first_entity()) {
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
                    let info = registry.type_info_by_id(*type_id).unwrap();
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
        } else if let Some(id) = selection.as_ref().and_then(|s| s.first_asset()) {
            let registry = AssetRegistry::get();
            if let Ok(asset) = registry.load_dyn_by_id(id) {
                if let Some(meta) = registry.asset_meta_from_id(id) {
                    if let Some(type_uuid) = meta.type_uuid {
                        if let Some(inspector) = self.asset_inspector_lookup(type_uuid) {
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

    fn type_inspector_lookup(&self, type_id: TypeId) -> Option<&dyn TypeInspector> {
        self.type_association
            .get(&type_id)
            .and_then(|id| self.type_inspectors.get(id))
            .and_then(|inspector| Some(inspector.as_ref()))
            .or_else(|| {
                self.type_inspectors
                    .get(&type_id)
                    .and_then(|inspector| Some(inspector.as_ref()))
            })
    }

    fn asset_inspector_lookup(&self, type_id: Uuid) -> Option<&dyn AssetInspector> {
        self.asset_inspectors
            .get(&type_id)
            .map(|inspector| inspector.as_ref())
    }

    fn show_inspector(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
    ) -> bool {
        let name = Self::display_name(instance);
        let mut remove = false;
        match self.type_inspector_lookup(instance.as_any().type_id()) {
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
        if let Some(inspector) = self.type_inspector_lookup(instance.as_any().type_id()) {
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
