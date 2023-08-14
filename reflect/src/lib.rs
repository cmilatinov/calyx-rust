use std::fmt::Debug;

pub mod types;
pub mod registry;
pub extern crate reflect_derive;
pub extern crate inventory;

#[cfg(test)]
mod tests {
    use crate as reflect;

    #[test]
    fn derive_reflect() {
        struct Test {
            testing: usize
        }

        #[derive(reflect_derive::Reflect)]
        struct MyStruct {
            test1: u32,
            test2: String,
            more: Test
        }

        let mut registry = reflect::registry::TypeRegistry::new();
        assert_eq!(registry.types.len(), 1);
    }
}
