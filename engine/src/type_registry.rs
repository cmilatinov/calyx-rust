use std::any::Any;
use inventory;
use crate::{singleton_with_init};
use crate::ecs::{Component, ComponentInfo};

pub struct TypeRegistry;

singleton_with_init!(TypeRegistry);

impl Default for TypeRegistry {
    fn default() -> Self {
        for comp in inventory::iter::<ComponentInfo> {
            println!("{:?}", comp.register);
        }
        Self
    }
}