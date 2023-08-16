use std::any::Any;
use reflect::registry::TypeRegistry;
use reflect::types::TypeInfo;
use reflect_derive::Reflect;

#[derive(Copy, Clone)]
struct Test {
    testing: usize
}

#[derive(Reflect)]
struct MyStruct {
    test1: u32,
    str: String,
    another_one: Test
}

fn main() {
    let registry = TypeRegistry::new();
    let mut instance: Box<dyn Any> = Box::new(MyStruct {
        test1: 420,
        str: String::from("deez nuts"),
        another_one: Test { testing: 69 }
    });
    let info = registry.type_info::<MyStruct>().unwrap();
    if let TypeInfo::Struct(ref ty) = info {
        let field = ty.field("str").unwrap();
        println!(
            "{}: {} = {:?}",
            field.name, field.type_name,
            field.get::<String>(instance.as_ref()).unwrap()
        );
        field.set::<String>(instance.as_mut(), String::from("69"));
        println!(
            "{}: {} = {:?}",
            field.name, field.type_name,
            field.get::<String>(instance.as_ref()).unwrap()
        );
    }
}

