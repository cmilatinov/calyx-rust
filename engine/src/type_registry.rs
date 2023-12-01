use crate::singleton;
use crate::utils::Init;
use reflect::type_registry::TypeRegistrationFn;
use std::ops::{Deref, DerefMut};

#[derive(Default)]
pub struct TypeRegistry(reflect::type_registry::TypeRegistry);

impl Deref for TypeRegistry {
    type Target = reflect::type_registry::TypeRegistry;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for TypeRegistry {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Init for TypeRegistry {
    fn initialize(&mut self) {
        for f in inventory::iter::<TypeRegistrationFn> {
            (f.0)(self);
        }
    }
}

singleton!(TypeRegistry);
