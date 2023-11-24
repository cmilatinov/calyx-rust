use reflect::type_registry::TypeRegistry;
use reflect::ReflectDefault;
use std::any::TypeId;
use std::collections::hash_map::Iter;
use std::collections::HashMap;
use utils::{singleton, type_ids, Init};

use crate::component::{Component, ReflectComponent};

#[derive(Default)]
pub struct ClassRegistry {
    components: HashMap<TypeId, Box<dyn Component>>,
}

impl Init for ClassRegistry {
    fn initialize(&mut self) {
        self.refresh_class_lists();
    }
}

singleton!(ClassRegistry);

impl ClassRegistry {
    pub fn components(&self) -> Iter<TypeId, Box<dyn Component>> {
        self.components.iter()
    }

    pub fn refresh_class_lists(&mut self) {
        self.components.clear();
        let registry = TypeRegistry::get();
        for type_id in registry.all_of(type_ids!(ReflectDefault, ReflectComponent)) {
            let meta_default = registry.trait_meta::<ReflectDefault>(type_id).unwrap();
            let meta_component = registry.trait_meta::<ReflectComponent>(type_id).unwrap();
            let instance = meta_default.default();
            let component = meta_component.get_boxed(instance).unwrap();
            self.components.insert(type_id, component);
        }
    }
}
