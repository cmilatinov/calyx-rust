use crate::{Reflect, TraitMeta, TraitMetaFrom};

#[derive(Clone)]
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
