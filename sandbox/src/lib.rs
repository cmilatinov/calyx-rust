mod test;
// mod tps_camera;

use engine::reflect::type_registry::TypeRegistry;
use engine::reflect::ReflectDefault;

pub struct ReflectRegistrationFn {
    pub name: &'static str,
    pub function: fn(&mut TypeRegistry),
}
engine::inventory::collect!(ReflectRegistrationFn);

#[no_mangle]
pub extern "C" fn plugin_main(registry: &mut TypeRegistry) {
    println!(
        "Loading sandbox: {:?}",
        std::any::TypeId::of::<ReflectDefault>()
    );
    for f in engine::inventory::iter::<ReflectRegistrationFn>() {
        println!("{}", f.name);
        (f.function)(registry);
    }
}
