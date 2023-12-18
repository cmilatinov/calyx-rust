extern crate inventory;
extern crate reflect_derive;

pub use self::impls::*;
pub use self::reflect::*;
pub use self::reflect_derive::*;
pub use self::trait_meta::*;
pub use self::type_info::*;
pub use self::type_uuid::*;

mod impls;
mod reflect;
mod trait_meta;
mod type_info;
pub mod type_registry;
pub mod type_uuid;

#[cfg(test)]
mod tests {
    use reflect::reflect_trait;
    use reflect::type_registry::TypeRegistry;
    use reflect::Reflect;
    use reflect::TypeInfo;

    use crate as reflect;

    #[test]
    fn test_reflection() {
        struct TestGeneric<T> {
            inner: T,
        }

        #[derive(Copy, Clone)]
        struct Test {
            testing: usize,
        }

        #[reflect_trait]
        pub trait TestTrait {
            fn do_something(&self) {
                println!("something");
            }
        }

        #[derive(Reflect)]
        #[reflect(TestTrait)]
        struct MyStruct {
            test_u32: u32,
            test_str: String,
            test_struct: Test,
            gen: TestGeneric<i32>,
        }

        impl TestTrait for MyStruct {
            fn do_something(&self) {
                println!("Hello from 'do_something' {}", self.test_u32);
            }
        }

        let mut registry = TypeRegistry::default();
        registry.register::<MyStruct>();
        let mut instance: Box<dyn Reflect> = Box::new(MyStruct {
            test_u32: 123,
            test_str: String::from("testing string"),
            test_struct: Test { testing: 321 },
            gen: TestGeneric::<i32> { inner: 32 },
        });

        assert_eq!(instance.type_name_short(), "MyStruct");
        assert_eq!(
            instance.type_name(),
            "reflect::tests::test_reflection::MyStruct"
        );

        let info = registry.type_info::<MyStruct>().unwrap();
        let trait_meta = registry
            .trait_meta::<ReflectTestTrait>(instance.as_ref().type_id())
            .unwrap();
        let tr = trait_meta.get(instance.as_ref()).unwrap();
        tr.do_something();
        if let TypeInfo::Struct(ref ty) = info {
            let field_str = ty.field("test_str").unwrap();
            assert_eq!(field_str.name, "test_str");
            assert_eq!(field_str.type_name, "alloc::string::String");
            assert_eq!(
                field_str.get::<String>(instance.as_ref()).unwrap(),
                "testing string"
            );

            field_str.set::<String>(instance.as_mut(), String::from("234"));

            assert_eq!(field_str.name, "test_str");
            assert_eq!(field_str.type_name, "alloc::string::String");
            assert_eq!(field_str.get::<String>(instance.as_ref()).unwrap(), "234");
        }
    }
}
