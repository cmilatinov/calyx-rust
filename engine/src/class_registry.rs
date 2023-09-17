use crate::component::{Component, ReflectComponent};
use reflect::type_registry::TypeRegistry;
use reflect::ReflectDefault;
use utils::{singleton, type_ids, Init};

#[derive(Default)]
pub struct ClassRegistry {
    components: Vec<Box<dyn Component>>,
}

impl Init for ClassRegistry {
    fn initialize(&mut self) {
        let registry = TypeRegistry::get();
        for type_id in registry.all_of(type_ids!(ReflectDefault, ReflectComponent)) {
            let meta_default = registry.trait_meta::<ReflectDefault>(type_id).unwrap();
            let meta_component = registry.trait_meta::<ReflectComponent>(type_id).unwrap();
            let instance = meta_default.default();
            let component = meta_component.get_boxed(instance).unwrap();
            self.components.push(component);
        }
    }
}

singleton!(ClassRegistry);

impl ClassRegistry {
    pub fn components(&self) -> &Vec<Box<dyn Component>> {
        &self.components
    }
}
