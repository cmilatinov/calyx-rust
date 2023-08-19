use reflect::registry::TypeRegistry;
use reflect::TypeInfo;
use reflect::Reflect;
use reflect::reflect_trait;

#[derive(Copy, Clone)]
struct Test {
    testing: usize
}

#[derive(Reflect)]
#[reflect(TestTrait)]
struct MyStruct {
    test1: u32,
    str: String,
    another_one: Test
}

#[reflect_trait]
pub trait TestTrait {
    fn do_something(&self) {
        println!("something");
    }
}

impl TestTrait for MyStruct {
    fn do_something(&self) {
        println!("Hello from 'do_something' {}", self.test1);
    }
}

fn main() {
    let mut registry = TypeRegistry::default();
    registry.register::<MyStruct>();
    let mut instance: Box<dyn Reflect> = Box::new(MyStruct {
        test1: 420,
        str: String::from("deez nuts"),
        another_one: Test { testing: 69 }
    });
    println!("{}", instance.type_name_short());
    println!("{}", instance.type_name());
    let info = registry.type_info::<MyStruct>().unwrap();
    let trait_meta = registry.trait_meta::<ReflectTestTrait>(instance.as_ref().type_id()).unwrap();
    let tr = trait_meta.get(instance.as_ref()).unwrap();
    tr.do_something();
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

