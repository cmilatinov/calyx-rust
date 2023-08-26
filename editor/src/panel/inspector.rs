use engine::egui::Ui;
use reflect::ReflectDefault;
use reflect::registry::TypeRegistry;
use crate::inspector::ReflectTypeInspector;
use crate::panel::Panel;

pub struct PanelInspector;

impl Default for PanelInspector {
    fn default() -> Self {
        let registry = TypeRegistry::get();
        let defaults = registry.list_types::<ReflectDefault>();
        let inspectors = registry.list_types::<ReflectTypeInspector>();
        let meta = registry.trait_meta::<ReflectDefault>(defaults[0]).unwrap();
        let instance = meta.default();
        println!("{}", instance.type_name());
        println!("{}", instance.type_name_short());
        Self
    }
}


impl Panel for PanelInspector {
    fn name() -> &'static str {
        "Inspector"
    }

    fn ui(&mut self, _ui: &mut Ui) {
        // TODO: Reflect needs to be fully implemented
    }
}
