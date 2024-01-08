use std::collections::hash_map::Iter;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use uuid::Uuid;

use reflect::{ReflectDefault, TypeUuid};

use crate::component::{Component, ReflectComponent};
use crate::type_registry::TypeRegistry;
use crate::utils::{singleton, type_uuids, Init};

#[derive(Default)]
pub struct ClassRegistry {
    components: HashMap<Uuid, Box<dyn Component>>,
}

impl Init for ClassRegistry {
    fn initialize(&mut self) {
        self.refresh_class_lists();
    }
}

singleton!(ClassRegistry);

impl ClassRegistry {
    pub fn component(&self, id: Uuid) -> Option<&dyn Component> {
        self.components.get(&id).map(|b| b.deref())
    }

    pub fn components(&self) -> Iter<Uuid, Box<dyn Component>> {
        self.components.iter()
    }

    pub fn refresh_class_lists(&mut self) {
        self.components.clear();
        let registry = TypeRegistry::get();
        for type_id in registry.all_of(type_uuids!(ReflectDefault, ReflectComponent)) {
            let meta_default = registry.trait_meta::<ReflectDefault>(type_id).unwrap();
            let meta_component = registry.trait_meta::<ReflectComponent>(type_id).unwrap();
            let instance = meta_default.default();
            let component = meta_component.get_boxed(instance).unwrap();
            self.components.insert(type_id, component);
        }
    }
}
