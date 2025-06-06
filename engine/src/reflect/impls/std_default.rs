use crate as engine;
use crate::reflect::{Reflect, TraitMeta, TraitMetaFrom};
use engine_derive::TypeUuid;

#[derive(Clone, TypeUuid)]
#[uuid = "1aebc41e-39f5-4921-8a53-ab0035bdbada"]
#[repr(C)]
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
