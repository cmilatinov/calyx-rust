use crate as reflect;
use crate::{Reflect, TraitMeta, TraitMetaFrom};
use reflect_derive::TypeUuid;

#[derive(Clone, TypeUuid)]
#[uuid = "d8a30f16-05ca-401d-95b2-a33808e2bcda"]
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
