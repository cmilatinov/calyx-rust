#![allow(missing_docs)]

use crate as calyx_engine;
use crate::reflect::{Reflect, TraitMeta, TraitMetaFrom};
use calyx_engine_derive::TypeUuid;

#[derive(Clone, TypeUuid)]
#[uuid = "1aebc41e-39f5-4921-8a53-ab0035bdbada"]
pub struct ReflectDefault {
    default: fn() -> Box<dyn Reflect>,
}

impl ReflectDefault {
    pub fn default(&self) -> Box<dyn Reflect> {
        (self.default)()
    }
}

impl TraitMeta for ReflectDefault {}

impl<T: Reflect + Default> TraitMetaFrom<T> for ReflectDefault {
    fn trait_meta() -> Self {
        ReflectDefault {
            default: || Box::<T>::default(),
        }
    }
}
